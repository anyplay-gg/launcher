//! ATRAC9 decoding with LibAtrac9, the reference decoder (vendored C in `libatrac9/`, MIT). This
//! is the only code that calls it; everything else uses [`Decoder`].
//!
//! LibAtrac9's bit reader does not check where a buffer ends, so a damaged stream could make it
//! read past the bytes it was given. [`Decoder::decode_superframe`] therefore copies every
//! superframe into a buffer with a long run of zeros after it, which no frame can read beyond.

use std::ffi::{c_int, c_short, c_uchar, c_void};
use std::fmt;
use std::ptr::NonNull;

#[repr(C)]
#[derive(Default)]
struct CodecInfo {
    channels: c_int,
    channel_config_index: c_int,
    sampling_rate: c_int,
    superframe_size: c_int,
    frames_in_superframe: c_int,
    frame_samples: c_int,
    wlength: c_int,
    config_data: [c_uchar; 4],
}

unsafe extern "C" {
    fn Atrac9GetHandle() -> *mut c_void;
    fn Atrac9ReleaseHandle(handle: *mut c_void);
    fn Atrac9InitDecoder(handle: *mut c_void, config: *mut c_uchar) -> c_int;
    fn Atrac9Decode(
        handle: *mut c_void,
        data: *const c_uchar,
        pcm: *mut c_short,
        bytes_used: *mut c_int,
    ) -> c_int;
    fn Atrac9GetCodecInfo(handle: *mut c_void, info: *mut CodecInfo) -> c_int;
}

/// Zeros after every superframe: more than one frame can ever read (a frame is at most 2 KiB).
const GUARD: usize = 4096;

/// A LibAtrac9 error code (`error_codes.h`), or 0 for a problem found on this side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(pub i32);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ATRAC9 error {:#x}", self.0)
    }
}

impl std::error::Error for Error {}

/// The stream's shape, from its four configuration bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Info {
    pub channels: usize,
    pub sample_rate: u32,
    pub superframe_size: usize,
    pub frames_in_superframe: usize,
    /// Samples per channel in one frame.
    pub frame_samples: usize,
}

impl Info {
    /// Interleaved samples one superframe decodes to.
    pub fn superframe_samples(&self) -> usize {
        self.channels * self.frame_samples * self.frames_in_superframe
    }
}

pub struct Decoder {
    handle: NonNull<c_void>,
    info: Info,
    scratch: Vec<u8>,
}

// The handle is plain memory owned by this value; LibAtrac9 keeps no global state.
unsafe impl Send for Decoder {}

impl Decoder {
    /// A decoder for the stream the configuration bytes (from the file's `fmt ` chunk) describe.
    pub fn new(mut config: [u8; 4]) -> Result<Self, Error> {
        // SAFETY: `Atrac9GetHandle` returns zeroed memory owned by the caller, or null.
        let handle = NonNull::new(unsafe { Atrac9GetHandle() }).ok_or(Error(0))?;
        let mut decoder = Self {
            handle,
            info: Info {
                channels: 0,
                sample_rate: 0,
                superframe_size: 0,
                frames_in_superframe: 0,
                frame_samples: 0,
            },
            scratch: Vec::new(),
        };
        // SAFETY: the handle is live; `config` is four readable bytes, as the API takes.
        let status = unsafe { Atrac9InitDecoder(handle.as_ptr(), config.as_mut_ptr()) };
        if status != 0 {
            return Err(Error(status));
        }
        let mut raw = CodecInfo::default();
        // SAFETY: the handle is initialised and `raw` has the C struct's layout.
        let status = unsafe { Atrac9GetCodecInfo(handle.as_ptr(), &mut raw) };
        if status != 0 {
            return Err(Error(status));
        }
        let info = Info {
            channels: usize::try_from(raw.channels).unwrap_or(0),
            sample_rate: u32::try_from(raw.sampling_rate).unwrap_or(0),
            superframe_size: usize::try_from(raw.superframe_size).unwrap_or(0),
            frames_in_superframe: usize::try_from(raw.frames_in_superframe).unwrap_or(0),
            frame_samples: usize::try_from(raw.frame_samples).unwrap_or(0),
        };
        let sane = (1..=8).contains(&info.channels)
            && info.sample_rate > 0
            && (1..=GUARD).contains(&info.superframe_size)
            && (1..=8).contains(&info.frames_in_superframe)
            && (1..=1024).contains(&info.frame_samples);
        if !sane {
            return Err(Error(0));
        }
        decoder.info = info;
        decoder.scratch = vec![0; info.superframe_size + GUARD];
        Ok(decoder)
    }

    pub fn info(&self) -> Info {
        self.info
    }

    /// Decodes one superframe into `pcm` (interleaved, [`Info::superframe_samples`] long).
    pub fn decode_superframe(&mut self, superframe: &[u8], pcm: &mut [i16]) -> Result<(), Error> {
        let info = self.info;
        if superframe.len() != info.superframe_size || pcm.len() < info.superframe_samples() {
            return Err(Error(0));
        }
        self.scratch[..superframe.len()].copy_from_slice(superframe);
        self.scratch[superframe.len()..].fill(0);
        let frame_len = info.channels * info.frame_samples;
        let mut offset = 0usize;
        for frame in 0..info.frames_in_superframe {
            if offset >= info.superframe_size {
                return Err(Error(0));
            }
            let out = &mut pcm[frame * frame_len..(frame + 1) * frame_len];
            let mut used: c_int = 0;
            // SAFETY: the handle is initialised; the input pointer has at least GUARD readable
            // bytes after it (the frame reads less); `out` holds one frame of interleaved samples,
            // which is what one call writes; `used` is a valid int.
            let status = unsafe {
                Atrac9Decode(
                    self.handle.as_ptr(),
                    self.scratch.as_ptr().add(offset),
                    out.as_mut_ptr(),
                    &mut used,
                )
            };
            if status != 0 {
                return Err(Error(status));
            }
            offset += usize::try_from(used).map_err(|_| Error(0))?;
        }
        Ok(())
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: the handle came from `Atrac9GetHandle` and is released once.
        unsafe { Atrac9ReleaseHandle(self.handle.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 48 kHz stereo, 48-byte frames, four to a superframe: a real theme song's configuration.
    const CONFIG: [u8; 4] = [0xFE, 0x74, 0x05, 0xF0];

    #[test]
    fn the_configuration_gives_the_streams_shape() {
        let decoder = Decoder::new(CONFIG).unwrap();
        assert_eq!(
            decoder.info(),
            Info {
                channels: 2,
                sample_rate: 48_000,
                superframe_size: 192,
                frames_in_superframe: 4,
                frame_samples: 256,
            }
        );
    }

    #[test]
    fn a_bad_configuration_is_an_error() {
        assert!(Decoder::new([0, 0, 0, 0]).is_err());
        assert!(Decoder::new([0xFE, 0xFF, 0xFF, 0xFF]).is_err());
    }

    #[test]
    fn noise_never_crashes_and_wrong_sizes_are_refused() {
        let mut decoder = Decoder::new(CONFIG).unwrap();
        let mut pcm = vec![0i16; decoder.info().superframe_samples()];
        let mut seed = 0x2545_F491u32;
        for _ in 0..2_000 {
            let noise: Vec<u8> = (0..192)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as u8
                })
                .collect();
            let _ = decoder.decode_superframe(&noise, &mut pcm);
        }
        assert!(decoder.decode_superframe(&[0; 100], &mut pcm).is_err());
        assert!(
            decoder
                .decode_superframe(&[0; 192], &mut pcm[..10])
                .is_err()
        );
    }
}
