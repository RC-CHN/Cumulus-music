use std::ffi::c_void;

unsafe extern "C" {
    pub fn netease_rotate_init(
        state: *mut c_void,
        tables: *const f32,
        count: usize,
        phase: f32,
    ) -> i32;
    pub fn netease_rotate_period(state: *mut c_void, seconds: f32) -> i32;
    pub fn netease_rotate_reset(state: *mut c_void);
    pub fn netease_rotate_process(state: *mut c_void, data: *mut f32, valid: *mut u32) -> i32;
    pub fn netease_rotate_phase(state: *const c_void) -> f32;
    pub fn netease_rotate_response(state: *const c_void) -> *const f32;
    pub fn netease_rotate_stage_init(
        state: *mut c_void,
        tables: *const f32,
        count: usize,
        capacity: u32,
    ) -> i32;
    pub fn netease_rotate_stage_period(state: *mut c_void, seconds: f32) -> i32;
    pub fn netease_rotate_stage_reset(state: *mut c_void);
    pub fn netease_rotate_stage_bypass(state: *mut c_void, bypass: u32) -> i32;
    pub fn netease_rotate_stage_process(
        state: *mut c_void,
        data: *mut f32,
        valid: *mut u32,
        end: *mut u32,
    ) -> i32;
    pub fn netease_rotate_stage_phase(state: *const c_void) -> f32;
    pub fn cumulus_dynamics_validate(data: *const u8, bytes: usize) -> i32;
    pub fn cumulus_multiband_init(
        state: *mut c_void,
        rate: f32,
        capacity: u32,
        config: *const u8,
        bytes: usize,
    ) -> i32;
    pub fn cumulus_multiband_set(state: *mut c_void, config: *const u8, bytes: usize) -> i32;
    pub fn netease_multiband_reset(state: *mut c_void) -> i32;
    pub fn netease_multiband_process(
        state: *mut c_void,
        main: *mut f32,
        side: *mut f32,
        valid: usize,
    ) -> i32;
    pub fn netease_multiband_history(state: *const c_void, history: *mut f32) -> i32;
    pub fn cumulus_dsp_size(kind: u32) -> usize;
    pub fn cumulus_dsp_alignment(kind: u32) -> usize;
    pub fn netease_biquad_coefficients(
        kind: u32,
        rate: f32,
        frequency: f32,
        gain: f32,
        q: f32,
        out: *mut f32,
    ) -> i32;
    pub fn netease_peq_coefficients(
        kind: u32,
        rate: f32,
        frequency: f32,
        gain: f32,
        q: f32,
        out: *mut f32,
    ) -> i32;
    pub fn netease_biquad_set(state: *mut c_void, coefficients: *const f32);
    pub fn netease_biquad_reset(state: *mut c_void);
    pub fn netease_biquad_process(state: *mut c_void, data: *mut f32, frames: usize);
    pub fn netease_geq_stage_init(
        state: *mut c_void,
        rate: f32,
        channels: u32,
        capacity: u32,
    ) -> i32;
    pub fn netease_geq_stage_set(state: *mut c_void, gains: *const f32) -> i32;
    pub fn netease_geq_stage_bypass(state: *mut c_void, bypass: u32) -> i32;
    pub fn netease_geq_stage_process(
        state: *mut c_void,
        data: *mut f32,
        frames: usize,
        dry: *mut f32,
    ) -> i32;
    pub fn netease_bt_init(state: *mut c_void, rate: f32, channels: u32, capacity: u32) -> i32;
    pub fn netease_bt_set(
        state: *mut c_void,
        bass: f32,
        treble: f32,
        low_frequency: f32,
        low_q: f32,
        high_frequency: f32,
        high_q: f32,
    ) -> i32;
    pub fn netease_bt_bypass(state: *mut c_void, bypass: u32) -> i32;
    pub fn netease_bt_process(
        state: *mut c_void,
        data: *mut f32,
        frames: usize,
        dry: *mut f32,
    ) -> i32;
    pub fn netease_peq_init(
        state: *mut c_void,
        rate: f32,
        channels: u32,
        capacity: u32,
        has_lfe: u32,
        process_lfe: u32,
    ) -> i32;
    pub fn netease_peq_band(
        state: *mut c_void,
        band: u32,
        kind: u32,
        gain: f32,
        frequency: f32,
        q: f32,
        enabled: u32,
    ) -> i32;
    pub fn netease_peq_set_gain(state: *mut c_void, gain: f32) -> i32;
    pub fn netease_peq_set_lfe(state: *mut c_void, enabled: u32) -> i32;
    pub fn netease_peq_bypass(state: *mut c_void, bypass: u32) -> i32;
    pub fn netease_peq_process(
        state: *mut c_void,
        data: *mut f32,
        frames: usize,
        dry: *mut f32,
    ) -> i32;
}
