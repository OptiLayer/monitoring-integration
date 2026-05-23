//! LZH session state machine.
//!
//! Drives the SPA→PCS dialogue: tracks where we are in the deposition process
//! and emits the correct [`OutFrame`] in response to incoming [`InFrame`]s and
//! host-supplied measurements.
//!
//! The state machine owns no I/O. The transport layer wraps it; tests drive it
//! directly. State transitions are anchored to observed pcap behavior; see
//! `tests::scenario_walks_full_layer_lifecycle` for the canonical timeline.

use crate::frame::{InFrame, OutFrame};

/// Per-layer recipe values pre-loaded into Spa (PCS does not push these).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerSpec {
    pub design_thickness: f64,
    pub design_rate: f64,
    pub n_index: u16,
    pub central_wavelength: f64,
}

/// Full coating recipe.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoatingPlan {
    pub layers: Vec<LayerSpec>,
    pub current_test_glass: u16,
}

/// Latest measurement supplied by the spectrometer / host.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Measurement {
    pub current_thickness: f64,
    pub current_rate: f64,
    pub mean_rate: f64,
    pub remaining_time: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Just started. Emitting Ready=F, waiting for host to call
    /// [`LzhSession::mark_initialized`].
    Initializing,
    /// Initialized or calibration done. Ready=T. Waiting for PCS to act.
    Ready,
    /// PCS pulsed AutoScaling; we are running Data Correction. Ready=F until
    /// the host calls [`LzhSession::mark_calibration_complete`].
    Calibrating,
    /// PCS asserted Deposit; we mirror Deposit+ACP and integrate thickness.
    Depositing,
    /// Layer target reached. TerminateLayer=T pulsed; waiting for PCS to drop
    /// Deposit before advancing.
    EndOfLayerPulse,
    /// All layers done. CoatingFinished=T held.
    Complete,
}

/// LZH protocol state machine. One per PCS connection.
#[derive(Debug)]
pub struct LzhSession {
    state: State,
    heartbeat: u16,
    plan: CoatingPlan,
    /// 1-based index into `plan.layers`. Zero before the first layer starts.
    current_layer: u16,
    measurement: Measurement,
    last_in_deposit: bool,
    last_in_acp: bool,
    last_in_auto_scaling: bool,
    in_frames_received: u64,
    out_frames_emitted: u64,
}

impl LzhSession {
    pub fn new(plan: CoatingPlan) -> Self {
        Self {
            state: State::Initializing,
            heartbeat: 0,
            plan,
            current_layer: 0,
            measurement: Measurement::default(),
            last_in_deposit: false,
            last_in_acp: false,
            last_in_auto_scaling: false,
            in_frames_received: 0,
            out_frames_emitted: 0,
        }
    }

    pub fn in_frames_received(&self) -> u64 {
        self.in_frames_received
    }

    pub fn out_frames_emitted(&self) -> u64 {
        self.out_frames_emitted
    }

    pub fn last_in_frame(&self) -> InFrame {
        InFrame {
            deposit: self.last_in_deposit,
            acp: self.last_in_acp,
            auto_scaling: self.last_in_auto_scaling,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn current_layer(&self) -> u16 {
        self.current_layer
    }

    pub fn heartbeat(&self) -> u16 {
        self.heartbeat
    }

    pub fn measurement(&self) -> Measurement {
        self.measurement
    }

    pub fn plan(&self) -> &CoatingPlan {
        &self.plan
    }

    /// Replace the active coating plan. Only allowed while the session has not
    /// started a real deposition — returns false otherwise.
    pub fn set_plan(&mut self, plan: CoatingPlan) -> bool {
        if !matches!(self.state, State::Initializing | State::Ready) {
            return false;
        }
        self.plan = plan;
        true
    }

    /// Host signals that startup is finished and we can advertise Ready=T.
    pub fn mark_initialized(&mut self) {
        if !matches!(self.state, State::Initializing) {
            tracing::debug!(state = ?self.state, "mark_initialized called outside Initializing; ignored");
            return;
        }
        self.transition(State::Ready, "host mark_initialized");
        self.current_layer = 1;
    }

    /// Host signals that Data Correction has finished. Returns to Ready.
    pub fn mark_calibration_complete(&mut self) {
        if !matches!(self.state, State::Calibrating) {
            tracing::debug!(state = ?self.state, "mark_calibration_complete called outside Calibrating; ignored");
            return;
        }
        self.transition(State::Ready, "host mark_calibration_complete");
    }

    /// Inject the latest spectrometer measurement. While Depositing, this also
    /// drives the end-of-layer decision: when `current_thickness` reaches the
    /// current layer's `design_thickness`, the state machine advances to
    /// [`State::EndOfLayerPulse`] on its own.
    pub fn update_measurement(&mut self, m: Measurement) {
        self.measurement = m;
        if !matches!(self.state, State::Depositing) {
            return;
        }
        let Some(spec) = self.current_layer_spec() else {
            return;
        };
        if m.current_thickness < spec.design_thickness {
            return;
        }
        self.transition(
            State::EndOfLayerPulse,
            "measurement reached design_thickness",
        );
    }

    /// Force the current layer to end without waiting for a measurement to
    /// cross the design threshold. Used when an external supervisor (e.g.
    /// OptiMonitor's optimization loop) makes the layer-done decision.
    pub fn force_end_of_layer(&mut self) {
        if !matches!(self.state, State::Depositing) {
            tracing::debug!(state = ?self.state, "force_end_of_layer ignored outside Depositing");
            return;
        }
        self.transition(State::EndOfLayerPulse, "host force_end_of_layer");
    }

    /// Consume an incoming PCS frame. Performs edge-triggered transitions.
    pub fn on_in_frame(&mut self, frame: InFrame) {
        self.in_frames_received = self.in_frames_received.saturating_add(1);

        let auto_scaling_rose = frame.auto_scaling && !self.last_in_auto_scaling;
        let auto_scaling_fell = !frame.auto_scaling && self.last_in_auto_scaling;
        let deposit_rose = frame.deposit && !self.last_in_deposit;
        let deposit_fell = !frame.deposit && self.last_in_deposit;
        let acp_changed = frame.acp != self.last_in_acp;
        self.last_in_auto_scaling = frame.auto_scaling;
        self.last_in_deposit = frame.deposit;
        self.last_in_acp = frame.acp;

        if auto_scaling_rose || auto_scaling_fell || deposit_rose || deposit_fell || acp_changed {
            tracing::info!(
                deposit = frame.deposit,
                acp = frame.acp,
                auto_scaling = frame.auto_scaling,
                state = ?self.state,
                "PCS flags changed",
            );
        }

        if auto_scaling_rose && matches!(self.state, State::Ready) {
            self.transition(State::Calibrating, "PCS asserted AutoScaling");
            return;
        }
        if deposit_rose && matches!(self.state, State::Ready) {
            self.transition(State::Depositing, "PCS asserted Deposit");
            return;
        }
        if !deposit_fell {
            return;
        }
        if !matches!(self.state, State::EndOfLayerPulse) {
            return;
        }
        self.advance_after_layer();
    }

    /// Produce the next outbound frame and tick the heartbeat. Call on the
    /// transport's emit schedule (observed ~4.6 Hz on the wire).
    pub fn emit(&mut self) -> OutFrame {
        self.heartbeat = self.heartbeat.wrapping_add(2);
        self.out_frames_emitted = self.out_frames_emitted.saturating_add(1);
        let mut f = OutFrame {
            heartbeat: self.heartbeat,
            current_test_glass: self.plan.current_test_glass,
            number_layers: self.plan.layers.len() as u16,
            layer: self.current_layer,
            ..Default::default()
        };
        self.apply_state_to_frame(&mut f);
        self.apply_measurement_to_frame(&mut f);
        self.apply_current_spec_to_frame(&mut f);
        f
    }

    fn apply_state_to_frame(&self, f: &mut OutFrame) {
        match self.state {
            State::Initializing => {}
            State::Ready => {
                f.ready = true;
            }
            State::Calibrating => {}
            State::Depositing => {
                f.ready = true;
                f.deposit = true;
                f.acp = true;
            }
            State::EndOfLayerPulse => {
                f.ready = true;
                f.terminate_layer = true;
                f.acp = true;
            }
            State::Complete => {
                f.ready = true;
                f.coating_finished = true;
            }
        }
    }

    fn apply_measurement_to_frame(&self, f: &mut OutFrame) {
        if !matches!(self.state, State::Depositing | State::EndOfLayerPulse) {
            return;
        }
        f.current_thickness = self.measurement.current_thickness;
        f.current_rate = self.measurement.current_rate;
        f.mean_rate = self.measurement.mean_rate;
        f.remaining_time = self.measurement.remaining_time;
    }

    fn apply_current_spec_to_frame(&self, f: &mut OutFrame) {
        let Some(spec) = self.current_layer_spec() else {
            return;
        };
        f.design_thickness = spec.design_thickness;
        f.design_rate = spec.design_rate;
        f.n_index = spec.n_index;
        f.central_wavelength = spec.central_wavelength;
    }

    fn current_layer_spec(&self) -> Option<LayerSpec> {
        if self.current_layer == 0 {
            return None;
        }
        self.plan
            .layers
            .get(self.current_layer as usize - 1)
            .copied()
    }

    /// Host signals that the whole process is over (e.g. OptiMonitor stops the
    /// chamber). Holds [`State::Complete`] until the session is rebuilt.
    pub fn mark_process_complete(&mut self) {
        if matches!(self.state, State::Complete) {
            return;
        }
        self.transition(State::Complete, "host mark_process_complete");
    }

    fn advance_after_layer(&mut self) {
        let total = self.plan.layers.len() as u16;
        // With an empty plan the host (e.g. OptiMonitor) drives completion
        // explicitly via mark_process_complete; never auto-finish here.
        if total > 0 && self.current_layer >= total {
            self.transition(State::Complete, "advance: last layer reached");
            return;
        }
        let next = self.current_layer + 1;
        self.current_layer = next;
        self.measurement = Measurement::default();
        self.transition(State::Ready, "advance: next layer");
    }

    fn transition(&mut self, next: State, reason: &'static str) {
        if self.state == next {
            return;
        }
        tracing::info!(
            from = ?self.state,
            to = ?next,
            layer = self.current_layer,
            heartbeat = self.heartbeat,
            reason,
            "LZH state transition",
        );
        self.state = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_layer_plan() -> CoatingPlan {
        CoatingPlan {
            layers: vec![LayerSpec {
                design_thickness: 291.0,
                design_rate: 0.2,
                n_index: 0,
                central_wavelength: 0.0,
            }],
            current_test_glass: 1,
        }
    }

    fn two_layer_plan() -> CoatingPlan {
        CoatingPlan {
            layers: vec![
                LayerSpec {
                    design_thickness: 291.0,
                    design_rate: 0.2,
                    n_index: 0,
                    central_wavelength: 0.0,
                },
                LayerSpec {
                    design_thickness: 150.0,
                    design_rate: 0.3,
                    n_index: 0,
                    central_wavelength: 0.0,
                },
            ],
            current_test_glass: 1,
        }
    }

    #[test]
    fn initial_emit_is_ready_false_no_magic_fields() {
        let mut s = LzhSession::new(one_layer_plan());
        let f = s.emit();
        assert!(!f.ready);
        assert!(!f.deposit);
        assert_eq!(f.layer, 0);
        assert_eq!(f.heartbeat, 2);
    }

    #[test]
    fn heartbeat_increments_by_two_per_emit() {
        let mut s = LzhSession::new(one_layer_plan());
        for expected in [2u16, 4, 6, 8, 10] {
            assert_eq!(s.emit().heartbeat, expected);
        }
    }

    #[test]
    fn mark_initialized_flips_ready_true_and_sets_layer_one() {
        let mut s = LzhSession::new(one_layer_plan());
        s.mark_initialized();
        let f = s.emit();
        assert!(f.ready);
        assert_eq!(f.layer, 1);
        assert_eq!(s.state(), State::Ready);
    }

    #[test]
    fn autoscaling_rising_edge_enters_calibrating() {
        let mut s = LzhSession::new(one_layer_plan());
        s.mark_initialized();
        s.on_in_frame(InFrame {
            auto_scaling: true,
            ..Default::default()
        });
        let f = s.emit();
        assert_eq!(s.state(), State::Calibrating);
        assert!(!f.ready);
    }

    #[test]
    fn calibration_complete_returns_to_ready() {
        let mut s = LzhSession::new(one_layer_plan());
        s.mark_initialized();
        s.on_in_frame(InFrame {
            auto_scaling: true,
            ..Default::default()
        });
        s.on_in_frame(InFrame {
            auto_scaling: false,
            ..Default::default()
        });
        s.mark_calibration_complete();
        assert_eq!(s.state(), State::Ready);
        assert!(s.emit().ready);
    }

    #[test]
    fn deposit_rising_edge_starts_depositing_and_mirrors_flags() {
        let mut s = LzhSession::new(one_layer_plan());
        s.mark_initialized();
        s.on_in_frame(InFrame {
            deposit: true,
            acp: true,
            ..Default::default()
        });
        let f = s.emit();
        assert_eq!(s.state(), State::Depositing);
        assert!(f.deposit);
        assert!(f.acp);
        assert!(f.ready);
        assert_eq!(f.design_thickness, 291.0);
        assert_eq!(f.design_rate, 0.2);
        assert_eq!(f.layer, 1);
    }

    #[test]
    fn measurement_below_target_does_not_terminate() {
        let mut s = LzhSession::new(one_layer_plan());
        s.mark_initialized();
        s.on_in_frame(InFrame {
            deposit: true,
            acp: true,
            ..Default::default()
        });
        s.update_measurement(Measurement {
            current_thickness: 100.0,
            current_rate: 0.3,
            mean_rate: 0.25,
            remaining_time: 600.0,
        });
        let f = s.emit();
        assert_eq!(s.state(), State::Depositing);
        assert!(!f.terminate_layer);
        assert_eq!(f.current_thickness, 100.0);
    }

    #[test]
    fn reaching_design_thickness_triggers_terminate_pulse() {
        let mut s = LzhSession::new(one_layer_plan());
        s.mark_initialized();
        s.on_in_frame(InFrame {
            deposit: true,
            acp: true,
            ..Default::default()
        });
        s.update_measurement(Measurement {
            current_thickness: 291.0,
            current_rate: 0.27,
            mean_rate: 0.26,
            remaining_time: 0.0,
        });
        let f = s.emit();
        assert_eq!(s.state(), State::EndOfLayerPulse);
        assert!(f.terminate_layer);
        assert!(!f.deposit);
        assert!(f.acp);
    }

    #[test]
    fn pcs_dropping_deposit_advances_layer() {
        let mut s = LzhSession::new(two_layer_plan());
        s.mark_initialized();
        s.on_in_frame(InFrame {
            deposit: true,
            acp: true,
            ..Default::default()
        });
        s.update_measurement(Measurement {
            current_thickness: 291.0,
            ..Default::default()
        });
        assert_eq!(s.state(), State::EndOfLayerPulse);

        s.on_in_frame(InFrame {
            deposit: false,
            acp: false,
            ..Default::default()
        });
        assert_eq!(s.state(), State::Ready);
        assert_eq!(s.current_layer(), 2);

        let f = s.emit();
        assert!(f.ready);
        assert!(!f.terminate_layer);
        assert_eq!(f.layer, 2);
        assert_eq!(f.design_thickness, 150.0);
        assert_eq!(f.design_rate, 0.3);
    }

    #[test]
    fn last_layer_completes_process() {
        let mut s = LzhSession::new(one_layer_plan());
        s.mark_initialized();
        s.on_in_frame(InFrame {
            deposit: true,
            acp: true,
            ..Default::default()
        });
        s.update_measurement(Measurement {
            current_thickness: 291.0,
            ..Default::default()
        });
        s.on_in_frame(InFrame {
            deposit: false,
            acp: false,
            ..Default::default()
        });
        assert_eq!(s.state(), State::Complete);
        let f = s.emit();
        assert!(f.coating_finished);
        assert!(f.ready);
    }

    #[test]
    fn scenario_walks_full_layer_lifecycle() {
        // Mirrors the timeline observed in deposition.pcapng for layer 1.
        let mut s = LzhSession::new(one_layer_plan());

        assert!(!s.emit().ready); // t≈0, Initializing
        s.mark_initialized(); // t≈4.5s

        let f = s.emit();
        assert!(f.ready);
        assert_eq!(f.layer, 1);

        // t≈26.5s: PCS pulses AutoScaling
        s.on_in_frame(InFrame {
            auto_scaling: true,
            ..Default::default()
        });
        assert_eq!(s.state(), State::Calibrating);
        assert!(!s.emit().ready);

        // t≈26.7s: AutoScaling drops while we are still calibrating
        s.on_in_frame(InFrame {
            auto_scaling: false,
            ..Default::default()
        });
        assert_eq!(s.state(), State::Calibrating);

        // t≈157s: Data Correction done
        s.mark_calibration_complete();
        assert!(s.emit().ready);

        // t≈867s: PCS opens shutter
        s.on_in_frame(InFrame {
            deposit: true,
            acp: true,
            ..Default::default()
        });
        let f = s.emit();
        assert!(f.deposit);
        assert!(f.acp);
        assert_eq!(f.design_thickness, 291.0);

        // mid-deposit measurement matching frame #125731
        s.update_measurement(Measurement {
            current_thickness: 289.732_356_731_548_6,
            current_rate: 0.265_906_699_220_280_76,
            mean_rate: 0.265_906_699_220_280_76,
            remaining_time: 3.017_248_332_473_524_6,
        });
        let f = s.emit();
        assert!(!f.terminate_layer);
        assert!((f.current_thickness - 289.732_356_731_548_6).abs() < 1e-9);

        // t≈1947.3s: thickness reaches target
        s.update_measurement(Measurement {
            current_thickness: 291.5,
            ..Default::default()
        });
        let f = s.emit();
        assert!(f.terminate_layer);
        assert!(!f.deposit);
        assert!(f.acp);

        // t≈1947.7s: PCS responds by closing shutter
        s.on_in_frame(InFrame {
            deposit: false,
            acp: false,
            ..Default::default()
        });
        assert_eq!(s.state(), State::Complete);
        assert!(s.emit().coating_finished);
    }
}
