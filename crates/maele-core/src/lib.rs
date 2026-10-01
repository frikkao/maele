//! Maele's audio/ASR front end.
//!
//! The `audio_toolkit` module is vendored from Handy
//! (<https://github.com/cjpais/Handy>, MIT, Copyright (c) 2025 CJ Pais) and
//! retained under the terms in `LICENSE-HANDY`. Handy's Tauri shell and its
//! 2.5k-line transcription manager are deliberately not vendored; Maele owns
//! the shell and a lean ASR wrapper.

pub mod asr;
pub mod audio_toolkit;

pub use asr::{
    default_path, fetch, fetch_all, models_dir, Asr, Bilingual, ModelSpec, Transcription, EN, LID,
    MODELS, NO,
};
pub use audio_toolkit::{
    list_input_devices, list_output_devices, read_wav_samples, save_wav_file, verify_wav_file,
    AudioRecorder, CpalDeviceInfo, VadPolicy,
};
