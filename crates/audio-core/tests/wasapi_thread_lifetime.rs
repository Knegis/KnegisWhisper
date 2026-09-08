#![cfg(target_os = "windows")]

/// Keep this in its own integration-test executable: the first CPAL caller
/// must be a thread which exits before another thread uses the audio host.
/// No microphone or output device is required. KNEG-178 crashed the hosted
/// process here rather than returning a Rust error when its COM owner exited.
#[test]
fn device_queries_survive_the_first_calling_thread_exiting() {
    for _ in 0..8 {
        let worker = std::thread::spawn(|| {
            let _ = kw_audio_core::capture::input_devices();
            let _ = kw_audio_core::system_audio::support();
        });
        assert!(
            worker.join().is_ok(),
            "device probing must survive each prior COM apartment exiting"
        );
    }
}
