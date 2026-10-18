//! D9: `wav16k` through symphonia + resampling, `aac` as AAC-LC 32 kbps mono `.m4a` through
//! Media Foundation's sink writer. Blocking; call inside `spawn_blocking`.
//!
//! The Media Foundation entry points are resolved at run time: a static import of `mfplat.dll`
//! or `mfreadwrite.dll` would keep Co-pet from starting on Windows N editions without the Media
//! Feature Pack, where only `aac` should fail.
use std::ffi::c_void;
use std::path::Path;
use std::sync::LazyLock;

use windows::core::{s, w, Interface, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Foundation::E_POINTER;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

use crate::error::{AppError, AppResult};
use crate::transcribe::audio::{discard, read_wav_f32, wav16k_from_any};

/// 16-bit mono WAV at 16 kHz, the speech engines' input. Failure is `audioConvert`.
pub fn wav16k(input: &Path, output: &Path) -> AppResult<()> {
    wav16k_from_any(input, output)
}

/// 32 kbps mono AAC in an `.m4a`, the archived audio. Failure is `audioConvert` (also on Windows
/// N editions without the Media Feature Pack); on any error no output file is left.
pub fn aac(input: &Path, output: &Path) -> AppResult<()> {
    let Some(mf) = MF.as_ref() else {
        eprintln!("[transcribe] aac {}: Media Foundation is not installed", output.display());
        return Err(AppError::Code("audioConvert"));
    };
    let tmp = output.with_extension("aac16k.wav");
    wav16k_from_any(input, &tmp)?;
    let samples = read_wav_f32(&tmp);
    discard(&tmp);
    let pcm: Vec<i16> = samples?.into_iter().map(|s| (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16).collect();
    encode(mf, &pcm, output).map_err(|e| {
        eprintln!("[transcribe] aac {}: {e}", output.display());
        discard(output);
        AppError::Code("audioConvert")
    })
}

/// What `GetProcAddress` returns: an untyped function pointer.
type Proc = unsafe extern "system" fn() -> isize;
type Startup = unsafe extern "system" fn(version: u32, flags: u32) -> HRESULT;
type Shutdown = unsafe extern "system" fn() -> HRESULT;
/// `MFCreateMediaType`, `MFCreateSample`.
type Create = unsafe extern "system" fn(out: *mut *mut c_void) -> HRESULT;
type CreateMemoryBuffer = unsafe extern "system" fn(max_length: u32, out: *mut *mut c_void) -> HRESULT;
type CreateSinkWriterFromUrl =
    unsafe extern "system" fn(url: PCWSTR, byte_stream: *mut c_void, attributes: *mut c_void, out: *mut *mut c_void) -> HRESULT;

/// The Media Foundation functions `encode` calls, with their Win32 signatures.
struct MediaFoundation {
    startup: Startup,
    shutdown: Shutdown,
    create_media_type: Create,
    create_memory_buffer: CreateMemoryBuffer,
    create_sample: Create,
    create_sink_writer_from_url: CreateSinkWriterFromUrl,
}

/// `mfplat.dll` and `mfreadwrite.dll` from System32, loaded once (never unloaded); `None` when
/// either DLL or one of the functions is missing.
static MF: LazyLock<Option<MediaFoundation>> = LazyLock::new(|| unsafe {
    use std::mem::transmute;
    let plat = LoadLibraryExW(w!("mfplat.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
    let readwrite = LoadLibraryExW(w!("mfreadwrite.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
    Some(MediaFoundation {
        startup: transmute::<Proc, Startup>(GetProcAddress(plat, s!("MFStartup"))?),
        shutdown: transmute::<Proc, Shutdown>(GetProcAddress(plat, s!("MFShutdown"))?),
        create_media_type: transmute::<Proc, Create>(GetProcAddress(plat, s!("MFCreateMediaType"))?),
        create_memory_buffer: transmute::<Proc, CreateMemoryBuffer>(GetProcAddress(plat, s!("MFCreateMemoryBuffer"))?),
        create_sample: transmute::<Proc, Create>(GetProcAddress(plat, s!("MFCreateSample"))?),
        create_sink_writer_from_url: transmute::<Proc, CreateSinkWriterFromUrl>(GetProcAddress(
            readwrite,
            s!("MFCreateSinkWriterFromURL"),
        )?),
    })
});

/// Calls a Media Foundation factory that returns a new COM object through its last argument.
unsafe fn create<T: Interface>(f: impl FnOnce(*mut *mut c_void) -> HRESULT) -> windows::core::Result<T> {
    let mut raw = std::ptr::null_mut();
    f(&mut raw).ok()?;
    if raw.is_null() {
        return Err(E_POINTER.into());
    }
    Ok(unsafe { T::from_raw(raw) })
}

/// Writes `pcm` (16 kHz mono) as AAC into an MPEG-4 container chosen by `output`'s extension.
fn encode(mf: &MediaFoundation, pcm: &[i16], output: &Path) -> windows::core::Result<()> {
    const RATE: u32 = 16_000;
    unsafe {
        // The worker converts on a blocking-pool thread: join (or reuse) the MTA.
        let com = CoInitializeEx(None, COINIT_MULTITHREADED);
        let r = (mf.startup)(MF_VERSION, MFSTARTUP_NOSOCKET).ok().and_then(|()| {
            let r = (|| {
                let url = HSTRING::from(output.as_os_str());
                let writer: IMFSinkWriter = create(|out| {
                    (mf.create_sink_writer_from_url)(PCWSTR(url.as_ptr()), std::ptr::null_mut(), std::ptr::null_mut(), out)
                })?;
                let out_type: IMFMediaType = create(|out| (mf.create_media_type)(out))?;
                out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                out_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
                out_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                out_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE)?;
                out_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 1)?;
                out_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 32_000 / 8)?;
                let stream = writer.AddStream(&out_type)?;
                let in_type: IMFMediaType = create(|out| (mf.create_media_type)(out))?;
                in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                in_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
                in_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                in_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE)?;
                in_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 1)?;
                in_type.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 2)?;
                in_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, RATE * 2)?;
                writer.SetInputMediaType(stream, &in_type, None)?;
                writer.BeginWriting()?;
                let mut t: i64 = 0;
                for chunk in pcm.chunks(RATE as usize) {
                    let bytes = (chunk.len() * 2) as u32;
                    let buf: IMFMediaBuffer = create(|out| (mf.create_memory_buffer)(bytes, out))?;
                    let mut p = std::ptr::null_mut();
                    buf.Lock(&mut p, None, None)?;
                    std::ptr::copy_nonoverlapping(chunk.as_ptr() as *const u8, p, bytes as usize);
                    buf.Unlock()?;
                    buf.SetCurrentLength(bytes)?;
                    let sample: IMFSample = create(|out| (mf.create_sample)(out))?;
                    sample.AddBuffer(&buf)?;
                    let dur = chunk.len() as i64 * 10_000_000 / i64::from(RATE);
                    sample.SetSampleTime(t)?;
                    sample.SetSampleDuration(dur)?;
                    writer.WriteSample(stream, &sample)?;
                    t += dur;
                }
                writer.Finalize()
            })();
            // Every Media Foundation object is released by now.
            let _ = (mf.shutdown)();
            r
        });
        if com.is_ok() {
            CoUninitialize();
        }
        r
    }
}
