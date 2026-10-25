//! Small safe wrappers around the CoreMediaIO C API: finding our device and its sink
//! stream, getting the stream's buffer queue and starting or stopping the stream.

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use objc2_core_foundation::{CFRetained, CFString};
use objc2_core_media::CMSimpleQueue;
use objc2_core_media_io::{
    CMIODeviceID, CMIODeviceStartStream, CMIODeviceStopStream, CMIOObjectGetPropertyData,
    CMIOObjectGetPropertyDataSize, CMIOObjectID, CMIOObjectPropertyAddress,
    CMIOStreamCopyBufferQueue, CMIOStreamID, kCMIODevicePropertyDeviceUID,
    kCMIODevicePropertyStreams, kCMIOHardwarePropertyDevices, kCMIOObjectPropertyElementMain,
    kCMIOObjectPropertyName, kCMIOObjectPropertyScopeGlobal, kCMIOObjectSystemObject,
    kCMIOStreamPropertyDirection,
};
use studio_core::error::{Result, StudioError};

use crate::{DEVICE_UUID, SINK_STREAM_NAME};

/// Value of `kCMIOStreamPropertyDirection` for an input stream (a sink). Output streams
/// report 0.
/// Data flowing out of the computer into the device: the sink an app writes into.
const DIRECTION_OUTPUT: u32 = 0;

/// The message shown when the extension is missing.
pub(crate) const NOT_INSTALLED: &str = "Webcam Studio Camera is not installed or not approved yet. Open the app's camera setup to install it.";

fn address(selector: u32) -> CMIOObjectPropertyAddress {
    CMIOObjectPropertyAddress {
        mSelector: selector,
        mScope: kCMIOObjectPropertyScopeGlobal,
        mElement: kCMIOObjectPropertyElementMain,
    }
}

/// Size in bytes of a property, or `None` if the object does not have it.
fn property_size(object: CMIOObjectID, selector: u32) -> Option<u32> {
    let address = address(selector);
    let mut size = 0u32;
    // SAFETY: `address` and `size` are valid for the duration of the call, and no
    // qualifier data is passed (size 0, null pointer).
    let status =
        unsafe { CMIOObjectGetPropertyDataSize(object, &address, 0, ptr::null(), &mut size) };
    (status == 0).then_some(size)
}

/// Reads a property into `buffer`, which must be at least `size` bytes. Returns the number
/// of bytes written.
fn read_property<T: Copy>(object: CMIOObjectID, selector: u32, buffer: &mut [T]) -> Option<usize> {
    let address = address(selector);
    let byte_len = std::mem::size_of_val(buffer);
    let mut used = 0u32;
    // SAFETY: `buffer` is valid for writes of `byte_len` bytes and `T` is plain data. The
    // other pointers are valid for the call, and no qualifier data is passed.
    let status = unsafe {
        CMIOObjectGetPropertyData(
            object,
            &address,
            0,
            ptr::null(),
            u32::try_from(byte_len).ok()?,
            &mut used,
            buffer.as_mut_ptr().cast::<c_void>(),
        )
    };
    (status == 0).then_some(used as usize)
}

/// Reads a property that is a list of 32-bit IDs.
fn read_ids(object: CMIOObjectID, selector: u32) -> Vec<u32> {
    let Some(size) = property_size(object, selector) else {
        return Vec::new();
    };
    let mut ids = vec![0u32; size as usize / std::mem::size_of::<u32>()];
    match read_property(object, selector, &mut ids) {
        Some(used) => {
            ids.truncate(used / std::mem::size_of::<u32>());
            ids
        }
        None => Vec::new(),
    }
}

/// Reads a property that is a single 32-bit number.
fn read_u32(object: CMIOObjectID, selector: u32) -> Option<u32> {
    let mut value = [0u32];
    read_property(object, selector, &mut value)?;
    Some(value[0])
}

/// Reads a property that is a string.
fn read_string(object: CMIOObjectID, selector: u32) -> Option<String> {
    let mut value: [*mut CFString; 1] = [ptr::null_mut()];
    read_property(object, selector, &mut value)?;
    let raw = NonNull::new(value[0])?;
    // SAFETY: CoreMediaIO returns string properties retained (+1), so we own one reference
    // and `CFRetained` releases it.
    let string = unsafe { CFRetained::from_raw(raw) };
    Some(string.to_string())
}

/// Finds the camera device with [`DEVICE_UUID`], if macOS lists it.
pub(crate) fn find_device() -> Option<CMIODeviceID> {
    read_ids(kCMIOObjectSystemObject, kCMIOHardwarePropertyDevices)
        .into_iter()
        .find(|&device| {
            read_string(device, kCMIODevicePropertyDeviceUID)
                .is_some_and(|uid| uid.eq_ignore_ascii_case(DEVICE_UUID))
        })
}

/// Finds the stream the app writes into (the extension's sink).
///
/// CoreMediaIO names directions from the computer's side: the camera picture meeting apps
/// read is an *input* stream (1), and the sink an app writes into is an *output* stream (0).
/// The stream called [`SINK_STREAM_NAME`] wins; otherwise the one output stream.
pub(crate) fn find_sink_stream(device: CMIODeviceID) -> Option<CMIOStreamID> {
    let streams: Vec<(CMIOStreamID, Option<String>, Option<u32>)> =
        read_ids(device, kCMIODevicePropertyStreams)
            .into_iter()
            .map(|stream| {
                (
                    stream,
                    read_string(stream, kCMIOObjectPropertyName),
                    read_u32(stream, kCMIOStreamPropertyDirection),
                )
            })
            .collect();
    pick_sink(&streams)
}

/// The sink among `(stream, name, direction)` triples. See [`find_sink_stream`].
fn pick_sink(streams: &[(CMIOStreamID, Option<String>, Option<u32>)]) -> Option<CMIOStreamID> {
    if let Some((stream, ..)) = streams
        .iter()
        .find(|(_, name, _)| name.as_deref() == Some(SINK_STREAM_NAME))
    {
        return Some(*stream);
    }
    let outputs: Vec<CMIOStreamID> = streams
        .iter()
        .filter(|(_, _, direction)| *direction == Some(DIRECTION_OUTPUT))
        .map(|(stream, ..)| *stream)
        .collect();
    match outputs.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

/// Called by CoreMediaIO when the queue changes. We do nothing: we only add buffers.
unsafe extern "C-unwind" fn queue_altered(
    _stream: CMIOStreamID,
    _token: *mut c_void,
    _ref_con: *mut c_void,
) {
}

/// Gets the buffer queue of `stream`. The caller owns the queue.
pub(crate) fn copy_buffer_queue(stream: CMIOStreamID) -> Result<CFRetained<CMSimpleQueue>> {
    let mut queue: *mut CMSimpleQueue = ptr::null_mut();
    // SAFETY: `queue_altered` has the right signature and ignores its arguments. The
    // reference constant is null and never used. `queue` is valid for one pointer write.
    let status = unsafe {
        CMIOStreamCopyBufferQueue(stream, Some(queue_altered), ptr::null_mut(), &mut queue)
    };
    if status != 0 {
        return Err(StudioError::Backend(format!(
            "could not get the camera's input queue (OSStatus {status})"
        )));
    }
    let queue = NonNull::new(queue)
        .ok_or_else(|| StudioError::Backend("the camera's input queue is missing".to_owned()))?;
    // SAFETY: The call succeeded, so `queue` holds a queue we own (+1, "Copy" rule).
    Ok(unsafe { CFRetained::from_raw(queue) })
}

/// Starts `stream` on `device`.
pub(crate) fn start_stream(device: CMIODeviceID, stream: CMIOStreamID) -> Result<()> {
    // SAFETY: Plain C call with two IDs; an invalid ID gives an error code.
    let status = unsafe { CMIODeviceStartStream(device, stream) };
    if status == 0 {
        Ok(())
    } else {
        Err(StudioError::Backend(format!(
            "could not start the camera's input stream (OSStatus {status})"
        )))
    }
}

/// Stops `stream` on `device`.
pub(crate) fn stop_stream(device: CMIODeviceID, stream: CMIOStreamID) -> Result<()> {
    // SAFETY: Plain C call with two IDs; an invalid ID gives an error code.
    let status = unsafe { CMIODeviceStopStream(device, stream) };
    if status == 0 {
        Ok(())
    } else {
        Err(StudioError::Backend(format!(
            "could not stop the camera's input stream (OSStatus {status})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE_NAME: &str = "Webcam Studio Camera";

    #[test]
    fn the_sink_is_found_by_its_name() {
        let streams = [
            (7, Some(SOURCE_NAME.to_string()), Some(1)),
            (9, Some(SINK_STREAM_NAME.to_string()), Some(0)),
        ];
        assert_eq!(pick_sink(&streams), Some(9));
    }

    #[test]
    fn without_names_the_output_stream_is_the_sink_never_the_camera_picture() {
        let streams = [(7, None, Some(1)), (9, None, Some(0))];
        assert_eq!(pick_sink(&streams), Some(9));
        // Only the picture stream: there is nowhere to write.
        assert_eq!(pick_sink(&[(7, None, Some(1))]), None);
        // Two unnamed outputs are ambiguous.
        assert_eq!(pick_sink(&[(7, None, Some(0)), (9, None, Some(0))]), None);
    }
}
