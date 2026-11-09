//! Small ownership boundary for the pinned sherpa 1.13.4 C API.
//! ABI: https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/sherpa-onnx/c-api/c-api.h
use kw_meeting_core::diarization::Turn;
use kw_notes_core::known_voices::{normalize, Sample, DIM};
use std::{
    collections::BTreeMap,
    ffi::{c_char, c_void, CString},
    path::Path,
};
type Handle = *const c_void;
type Destroy = unsafe extern "C" fn(Handle);
#[repr(C)]
struct Config {
    model: *const c_char,
    threads: i32,
    debug: i32,
    provider: *const c_char,
}
struct Owned {
    ptr: Handle,
    destroy: Destroy,
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.ptr) }
    }
}

pub fn extract(
    library: &Path,
    model: &Path,
    wav: &Path,
    turns: &[Turn],
) -> Result<Vec<Sample>, String> {
    #[cfg(not(windows))]
    {
        let _ = (library, model, wav, turns);
        Err("Known voices currently requires Windows".into())
    }
    #[cfg(windows)]
    // All handles and symbol borrows are scoped inside this library's lifetime.
    // Only our installed pinned path is supplied; dependencies use the DLL's
    // directory and system directories, never the working directory or PATH.
    unsafe {
        // Preflight the matching runtime before sherpa can dereference its API.
        // Keep this library loaded until after every sherpa object is destroyed.
        let root = library.parent().ok_or("Missing runtime directory")?;
        let ort: libloading::Library = libloading::os::windows::Library::load_with_flags(
            root.join("onnxruntime.dll"),
            0x100 | 0x1000,
        )
        .map_err(|e| e.to_string())?
        .into();
        #[repr(C)]
        struct ApiBase {
            get_api: unsafe extern "system" fn(u32) -> Handle,
            version: unsafe extern "system" fn() -> *const c_char,
        }
        let get_base = ort
            .get::<unsafe extern "system" fn() -> *const ApiBase>(b"OrtGetApiBase\0")
            .map_err(|e| e.to_string())?;
        let base = get_base();
        if base.is_null() || ((*base).get_api)(27).is_null() {
            return Err(
                "The installed ONNX runtime is incompatible; repair the speaker runtime".into(),
            );
        }
        let lib: libloading::Library =
            libloading::os::windows::Library::load_with_flags(library, 0x100 | 0x1000)
                .map_err(|e| e.to_string())?
                .into();
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {
                *lib.get::<$ty>(concat!($name, "\0").as_bytes())
                    .map_err(|e| e.to_string())?
            };
        }
        let version = symbol!(
            "SherpaOnnxGetVersionStr",
            unsafe extern "C" fn() -> *const c_char
        );
        let version = version();
        if version.is_null() || std::ffi::CStr::from_ptr(version).to_bytes() != b"1.13.4" {
            return Err("Speaker runtime version does not match this build".into());
        }
        let create = symbol!(
            "SherpaOnnxCreateSpeakerEmbeddingExtractor",
            unsafe extern "C" fn(*const Config) -> Handle
        );
        let destroy = symbol!("SherpaOnnxDestroySpeakerEmbeddingExtractor", Destroy);
        let dim = symbol!(
            "SherpaOnnxSpeakerEmbeddingExtractorDim",
            unsafe extern "C" fn(Handle) -> i32
        );
        let stream = symbol!(
            "SherpaOnnxSpeakerEmbeddingExtractorCreateStream",
            unsafe extern "C" fn(Handle) -> Handle
        );
        let destroy_stream = symbol!("SherpaOnnxDestroyOnlineStream", Destroy);
        let accept = symbol!(
            "SherpaOnnxOnlineStreamAcceptWaveform",
            unsafe extern "C" fn(Handle, i32, *const f32, i32)
        );
        let finish = symbol!(
            "SherpaOnnxOnlineStreamInputFinished",
            unsafe extern "C" fn(Handle)
        );
        let ready = symbol!(
            "SherpaOnnxSpeakerEmbeddingExtractorIsReady",
            unsafe extern "C" fn(Handle, Handle) -> i32
        );
        let compute = symbol!(
            "SherpaOnnxSpeakerEmbeddingExtractorComputeEmbedding",
            unsafe extern "C" fn(Handle, Handle) -> *const f32
        );
        let free = symbol!(
            "SherpaOnnxSpeakerEmbeddingExtractorDestroyEmbedding",
            unsafe extern "C" fn(*const f32)
        );
        let model =
            CString::new(model.to_str().ok_or("Invalid model path")?).map_err(|e| e.to_string())?;
        let config = Config {
            model: model.as_ptr(),
            threads: 1,
            debug: 0,
            provider: c"cpu".as_ptr(),
        };
        let ptr = create(&config);
        if ptr.is_null() {
            return Err("Could not initialize speaker extractor".into());
        }
        let extractor = Owned { ptr, destroy };
        if dim(extractor.ptr) != DIM as i32 {
            return Err("Incompatible speaker embedding dimension".into());
        }
        let mut reader = hound::WavReader::open(wav).map_err(|e| e.to_string())?;
        let spec = reader.spec();
        if spec.sample_rate != 16000
            || spec.channels != 1
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
        {
            return Err("Known voices needs a mono 16 kHz PCM16 meeting recording".into());
        }
        let duration = f64::from(reader.duration()) / 16000.0;
        let mut clusters: BTreeMap<u32, (Vec<f32>, f64)> = BTreeMap::new();
        for (index, turn) in turns.iter().enumerate() {
            if !turn.start_seconds.is_finite()
                || !turn.end_seconds.is_finite()
                || turn.start_seconds < 0.0
                || turn.end_seconds > duration
                || turn.end_seconds <= turn.start_seconds
            {
                continue;
            }
            // Reject overlapping turns, including same-cluster duplicates.
            if turns.iter().enumerate().any(|(other, t)| {
                index != other
                    && t.start_seconds < turn.end_seconds
                    && t.end_seconds > turn.start_seconds
            }) {
                continue;
            }
            let mut start = turn.start_seconds;
            while start + 1.0 <= turn.end_seconds {
                let end = (start + 30.0).min(turn.end_seconds);
                let count = ((end - start) * 16000.0) as usize;
                reader
                    .seek((start * 16000.0) as u32)
                    .map_err(|e| e.to_string())?;
                let samples: Vec<f32> = reader
                    .samples::<i16>()
                    .take(count)
                    .map(|s| s.map(|v| f32::from(v) / 32768.0))
                    .collect::<Result<_, _>>()
                    .map_err(|e| e.to_string())?;
                if samples.len() != count {
                    return Err("Truncated meeting recording".into());
                }
                let ptr = stream(extractor.ptr);
                if ptr.is_null() {
                    return Err("Could not allocate speaker stream".into());
                }
                let stream = Owned {
                    ptr,
                    destroy: destroy_stream,
                };
                accept(stream.ptr, 16000, samples.as_ptr(), samples.len() as i32);
                finish(stream.ptr);
                if ready(extractor.ptr, stream.ptr) != 0 {
                    let result = compute(extractor.ptr, stream.ptr);
                    if result.is_null() {
                        return Err("Speaker extraction returned no vector".into());
                    }
                    let vector = normalize(std::slice::from_raw_parts(result, DIM));
                    free(result);
                    if let Some(vector) = vector {
                        let entry = clusters
                            .entry(turn.speaker)
                            .or_insert_with(|| (vec![0.0; DIM], 0.0));
                        for (sum, value) in entry.0.iter_mut().zip(vector) {
                            *sum += value * (end - start) as f32;
                        }
                        entry.1 += end - start;
                    }
                }
                start = end;
            }
        }
        Ok(clusters
            .into_iter()
            .filter_map(|(speaker, (vector, seconds))| {
                Some(Sample {
                    speaker_id: format!("speaker_{speaker}"),
                    embedding: normalize(&vector)?,
                    seconds,
                })
            })
            .collect())
    }
}
