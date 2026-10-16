// Copyright 2025 LiveKit, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::impl_thread_safety;

#[cxx::bridge(namespace = "livekit_ffi")]
pub mod ffi {
    unsafe extern "C++" {
        include!("livekit/apm.h");

        type AudioProcessingModule;

        unsafe fn process_stream(
            self: Pin<&mut AudioProcessingModule>,
            src: *const i16,
            src_len: usize,
            dst: *mut i16,
            dst_len: usize,
            sample_rate: i32,
            num_channels: i32,
        ) -> i32;

        unsafe fn process_reverse_stream(
            self: Pin<&mut AudioProcessingModule>,
            src: *const i16,
            src_len: usize,
            dst: *mut i16,
            dst_len: usize,
            sample_rate: i32,
            num_channels: i32,
        ) -> i32;

        fn set_stream_delay_ms(self: Pin<&mut AudioProcessingModule>, delay: i32) -> i32;

        fn create_apm(
            echo_canceller_enabled: bool,
            gain_controller_enabled: bool,
            high_pass_filter_enabled: bool,
            noise_suppression_enabled: bool,
        ) -> UniquePtr<AudioProcessingModule>;

        fn create_gain_apm(
            adaptive: bool,
            initial_gain_db: f32,
            fixed_gain_db: f32,
            max_gain_change_db_per_second: f32,
        ) -> UniquePtr<AudioProcessingModule>;
    }
}

impl_thread_safety!(ffi::AudioProcessingModule, Send + Sync);

/// A gain-only AGC2 module, so callers do not need the cxx crate themselves.
pub struct GainProcessor {
    handle: cxx::UniquePtr<ffi::AudioProcessingModule>,
}

impl GainProcessor {
    pub fn new(adaptive: bool, initial_gain_db: f32, fixed_gain_db: f32, max_gain_change_db_per_second: f32) -> Self {
        Self { handle: ffi::create_gain_apm(adaptive, initial_gain_db, fixed_gain_db, max_gain_change_db_per_second) }
    }

    /// Processes one 10 ms mono frame in place; returns false when the module is missing or rejects the frame.
    pub fn process(&mut self, frame: &mut [i16], sample_rate: i32) -> bool {
        if self.handle.is_null() {
            return false;
        }
        let length = frame.len();
        let pointer = frame.as_mut_ptr();
        unsafe { self.handle.pin_mut().process_stream(pointer, length, pointer, length, sample_rate, 1) == 0 }
    }
}
