//! Bindings for [`AMidiDevice`], [`AMidiInputPort`], and [`AMidiOutputPort`]
//!
//! See [the NDK guide](https://developer.android.com/ndk/guides/audio/midi) for
//! design and usage instructions, and [the NDK reference](https://developer.android.com/ndk/reference/group/midi)
//! for an API overview.
//!
//! [`AMidiDevice`]: https://developer.android.com/ndk/reference/group/midi#amididevice
//! [`AMidiInputPort`]: https://developer.android.com/ndk/reference/group/midi#amidiinputport
//! [`AMidiOutputPort`]: https://developer.android.com/ndk/reference/group/midi#amidioutputport
#![cfg(feature = "midi")]

use super::media_error::{construct, MediaError, Result};

use std::fmt;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::ops::Deref;
use std::os::raw::{c_int, c_uint};
use std::ptr::{self, NonNull};
use std::sync::Arc;

/// Result of [`MidiOutputPort::receive()`].
#[derive(Copy, Clone, Debug)]
pub enum MidiOpcode {
    /// No MIDI messages are available.
    NoMessage,
    /// Received a MIDI message with the given length and the timestamp.
    Data {
        /// The length of the received message. The caller should limit the passed `buffer` slice to
        /// this length after [`MidiOutputPort::receive()`] returns.
        /// ```no_run
        /// use ndk::midi::{MidiOutputPort, MidiOpcode};
        ///
        /// let output_port: MidiOutputPort = todo!();
        /// let mut buffer = [0u8; 128];
        /// if let Ok(MidiOpcode::Data { length, .. }) = output_port.receive(&mut buffer) {
        ///     // process message
        ///     if let [message, key, velocity] = &buffer[..length] {
        ///         if message & 0xF0 == 0x90 { /* Note On message */ }
        ///         else if message & 0xF0 == 0x80 { /* Note Off message */ }
        ///         else { /*  */ }
        ///     }
        /// }
        /// ```
        length: usize,
        /// The timestamp associated with the message. This is consistent with the value returned by
        /// [`java.lang.System.nanoTime()`].
        ///
        /// [`java.lang.System.nanoTime()`]: https://developer.android.com/reference/java/lang/System#nanoTime()
        timestamp: i64,
    },
    /// Instructed to discard all pending MIDI data.
    Flush,
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MidiDeviceType {
    Bluetooth,
    USB,
    Virtual,
    Unknown(u32),
}

impl From<u32> for MidiDeviceType {
    fn from(value: u32) -> Self {
        match value {
            ffi::AMIDI_DEVICE_TYPE_BLUETOOTH => Self::Bluetooth,
            ffi::AMIDI_DEVICE_TYPE_USB => Self::USB,
            ffi::AMIDI_DEVICE_TYPE_VIRTUAL => Self::Virtual,
            v => Self::Unknown(v),
        }
    }
}

impl From<MidiDeviceType> for u32 {
    fn from(val: MidiDeviceType) -> Self {
        match val {
            MidiDeviceType::Bluetooth => ffi::AMIDI_DEVICE_TYPE_BLUETOOTH,
            MidiDeviceType::USB => ffi::AMIDI_DEVICE_TYPE_USB,
            MidiDeviceType::Virtual => ffi::AMIDI_DEVICE_TYPE_VIRTUAL,
            MidiDeviceType::Unknown(v) => v,
        }
    }
}

/// A thin wrapper over [`ffi::AMidiDevice`]. Please use [`MidiDevice`] if you need the thread-safe
/// version.
#[derive(Debug)]
pub struct UnsafeMidiDevice {
    ptr: NonNull<ffi::AMidiDevice>,
}

impl UnsafeMidiDevice {
    /// Assumes ownership of `ptr`
    ///
    /// # Safety
    /// `ptr` must be a valid pointer to an Android [`ffi::AMidiDevice`]. The calling thread also
    /// must be attached to the Java VM during the lifetime of the returned instance.
    pub unsafe fn from_ptr(ptr: NonNull<ffi::AMidiDevice>) -> Self {
        Self { ptr }
    }

    pub fn ptr(&self) -> NonNull<ffi::AMidiDevice> {
        self.ptr
    }

    /// Connects a native Midi Device object to the associated Java MidiDevice object.
    ///
    /// Use the returned [`UnsafeMidiDevice`] to access the rest of the native MIDI API.
    ///
    /// # Safety
    /// `env` and `midi_device_obj` must be valid pointers to a [`jni_sys::JNIEnv`] instance and a
    /// Java [`MidiDevice`](https://developer.android.com/reference/android/media/midi/MidiDevice)
    /// instance. The calling thread also must be attached to the Java VM during the lifetime of the
    /// returned instance.
    pub unsafe fn from_java(
        env: *mut jni_sys::JNIEnv,
        midi_device_obj: jni_sys::jobject,
    ) -> Result<UnsafeMidiDevice> {
        unsafe {
            let ptr = construct(|res| ffi::AMidiDevice_fromJava(env, midi_device_obj, res))?;
            Ok(Self::from_ptr(NonNull::new_unchecked(ptr)))
        }
    }

    /// Gets the number of input (sending) ports available on this device.
    pub fn num_input_ports(&self) -> Result<usize> {
        let num_input_ports = unsafe { ffi::AMidiDevice_getNumInputPorts(self.ptr.as_ptr()) };
        if num_input_ports >= 0 {
            Ok(num_input_ports as usize)
        } else {
            Err(MediaError::from_status(ffi::media_status_t(num_input_ports as c_int)).unwrap_err())
        }
    }

    /// Gets the number of output (receiving) ports available on this device.
    pub fn num_output_ports(&self) -> Result<usize> {
        let num_output_ports = unsafe { ffi::AMidiDevice_getNumOutputPorts(self.ptr.as_ptr()) };
        if num_output_ports >= 0 {
            Ok(num_output_ports as usize)
        } else {
            Err(
                MediaError::from_status(ffi::media_status_t(num_output_ports as c_int))
                    .unwrap_err(),
            )
        }
    }

    /// Gets the MIDI device type of this device.
    pub fn device_type(&self) -> Result<MidiDeviceType> {
        let device_type = unsafe { ffi::AMidiDevice_getType(self.ptr.as_ptr()) };
        if device_type >= 0 {
            Ok(MidiDeviceType::from(device_type as u32))
        } else {
            Err(MediaError::from_status(ffi::media_status_t(device_type)).unwrap_err())
        }
    }

    /// Opens the input port so that the client can send data to it.
    pub fn open_input_port(&self, port_number: i32) -> Result<NonNull<ffi::AMidiInputPort>> {
        unsafe {
            let input_port =
                construct(|res| ffi::AMidiInputPort_open(self.ptr.as_ptr(), port_number, res))?;
            Ok(NonNull::new_unchecked(input_port))
        }
    }

    /// Opens the output port so that the client can receive data from it.
    pub fn open_output_port(&self, port_number: i32) -> Result<NonNull<ffi::AMidiOutputPort>> {
        unsafe {
            let output_port =
                construct(|res| ffi::AMidiOutputPort_open(self.ptr.as_ptr(), port_number, res))?;
            Ok(NonNull::new_unchecked(output_port))
        }
    }
}

impl Drop for UnsafeMidiDevice {
    fn drop(&mut self) {
        let status = unsafe { ffi::AMidiDevice_release(self.ptr.as_ptr()) };
        MediaError::from_status(status).unwrap();
    }
}

/// The owner of [`MidiDevice`]. [`MidiDevice`], [`MidiInputPort`], and
/// [`MidiOutputPort`] holds an [`Arc<MidiDeviceGuard>`]. to ensure that the underlying
/// [`MidiDevice`] is not dropped while the safe wrappers are alive.
///
/// [`MidiDeviceGuard`] also holds a pointer to the current Java VM to attach the calling thread
/// of [`MidiDeviceGuard::drop()`] to the VM, which is required by [`ffi::AMidiDevice_release()`].
struct MidiDeviceGuard {
    midi_device: ManuallyDrop<UnsafeMidiDevice>,
    java_vm: NonNull<jni_sys::JavaVM>,
}

// SAFETY: [`MidiDeviceGuard::drop()`] attaches the calling thread to the Java VM if required.
unsafe impl Send for MidiDeviceGuard {}

// SAFETY: [`MidiDeviceGuard::drop()`] attaches the calling thread to the Java VM if required.
unsafe impl Sync for MidiDeviceGuard {}

impl Drop for MidiDeviceGuard {
    fn drop(&mut self) {
        unsafe {
            let java_vm_functions = self.java_vm.as_mut().as_ref().unwrap_unchecked();
            let java_vm = self.java_vm.as_ptr();
            let mut current_thread_was_attached = true;
            let mut env = MaybeUninit::uninit();

            // Try to get the current thread's JNIEnv
            if (java_vm_functions.GetEnv.unwrap_unchecked())(
                java_vm,
                env.as_mut_ptr(),
                jni_sys::JNI_VERSION_1_6,
            ) != jni_sys::JNI_OK
            {
                // Current thread is not attached to the Java VM. Try to attach.
                current_thread_was_attached = false;
                if (java_vm_functions
                    .AttachCurrentThreadAsDaemon
                    .unwrap_unchecked())(
                    java_vm, env.as_mut_ptr(), ptr::null_mut()
                ) != jni_sys::JNI_OK
                {
                    panic!("failed to attach the current thread to the Java VM");
                }
            }

            // Dropping MidiDevice requires the current thread to be attached to the Java VM.
            ManuallyDrop::drop(&mut self.midi_device);

            // Releasing MidiDevice is complete; if the current thread was not attached to the VM,
            // detach the current thread.
            if !current_thread_was_attached {
                (java_vm_functions.DetachCurrentThread.unwrap_unchecked())(java_vm);
            }
        }
    }
}

/// A thread-safe wrapper over [`MidiDevice`].
pub struct MidiDevice {
    guard: Arc<MidiDeviceGuard>,
}

impl MidiDevice {
    /// Assumes ownership of `ptr`
    ///
    /// # Safety
    /// `env` and `ptr` must be valid pointers to a [`jni_sys::JNIEnv`] instance and an Android
    /// [`ffi::AMidiDevice`].
    pub unsafe fn from_ptr(env: *mut jni_sys::JNIEnv, ptr: NonNull<ffi::AMidiDevice>) -> Self {
        Self::from_unsafe(env, UnsafeMidiDevice::from_ptr(ptr))
    }

    /// Connects a native Midi Device object to the associated Java MidiDevice object.
    ///
    /// Use the returned [`MidiDevice`] to access the rest of the native MIDI API.
    ///
    /// # Safety
    /// `env` and `midi_device_obj` must be valid pointers to a [`jni_sys::JNIEnv`] instance and a
    /// Java [`MidiDevice`](https://developer.android.com/reference/android/media/midi/MidiDevice)
    /// instance.
    pub unsafe fn from_java(
        env: *mut jni_sys::JNIEnv,
        midi_device_obj: jni_sys::jobject,
    ) -> Result<Self> {
        Ok(Self::from_unsafe(
            env,
            UnsafeMidiDevice::from_java(env, midi_device_obj)?,
        ))
    }

    /// Wraps the given unsafe [`MidiDevice`] instance into a safe counterpart.
    ///
    /// # Safety
    ///
    /// `env` must be a valid pointer to a [`jni_sys::JNIEnv`] instance.
    pub unsafe fn from_unsafe(env: *mut jni_sys::JNIEnv, midi_device: UnsafeMidiDevice) -> Self {
        let env_functions = env.as_mut().unwrap().as_ref().unwrap_unchecked();
        let mut java_vm = MaybeUninit::uninit();
        if (env_functions.GetJavaVM.unwrap_unchecked())(env, java_vm.as_mut_ptr())
            != jni_sys::JNI_OK
        {
            panic!("failed to get the current Java VM");
        }
        let java_vm = NonNull::new_unchecked(java_vm.assume_init());

        MidiDevice {
            guard: Arc::new(MidiDeviceGuard {
                midi_device: ManuallyDrop::new(midi_device),
                java_vm,
            }),
        }
    }

    /// Gets the number of input (sending) ports available on this device.
    pub fn num_input_ports(&self) -> Result<usize> {
        self.guard.midi_device.num_input_ports()
    }

    /// Gets the number of output (receiving) ports available on this device.
    pub fn num_output_ports(&self) -> Result<usize> {
        self.guard.midi_device.num_output_ports()
    }

    /// Gets the MIDI device type of this device.
    pub fn device_type(&self) -> Result<MidiDeviceType> {
        self.guard.midi_device.device_type()
    }

    /// Opens the input port so that the client can send data to it.
    pub fn open_input_port(&self, port_number: i32) -> Result<MidiInputPort> {
        Ok(MidiInputPort {
            // Convert the returned MidiInputPort<'_> into a MidiInputPort<'static>.
            //
            // SAFETY: the associated MIDI device of the input port will be alive during the
            // lifetime of the returned MidiInputPort because it is hold by _device.
            // Since Rust calls the destructor of fields in declaration order, the MIDI device
            // will be alive even when the input port is being dropped.
            inner: unsafe {
                UnsafeMidiInputPort::from_ptr(self.guard.midi_device.open_input_port(port_number)?)
            },
            _guard: Arc::clone(&self.guard),
        })
    }

    /// Opens the output port so that the client can receive data from it.
    pub fn open_output_port(&self, port_number: i32) -> Result<MidiOutputPort> {
        Ok(MidiOutputPort {
            // Convert the returned MidiInputPort<'_> into a MidiInputPort<'static>.
            //
            // SAFETY: the associated MIDI device of the output port will be alive during the
            // lifetime of the returned MidiOutputPort because it is hold by _device.
            // Since Rust calls the destructor of fields in declaration order, the MIDI device
            // will be alive even when the output port is being dropped.
            inner: unsafe {
                UnsafeMidiOutputPort::from_ptr(
                    self.guard.midi_device.open_output_port(port_number)?,
                )
            },
            _guard: Arc::clone(&self.guard),
        })
    }
}

impl fmt::Debug for MidiDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MidiDevice")
            .field("ptr", &self.guard.midi_device.ptr)
            .field("java_vm", &self.guard.java_vm)
            .finish()
    }
}

/// A thin wrapper over [`ffi::AMidiInputPort`]. Please use [`MidiInputPort`] if you need the
/// thread-safe version.
#[derive(Debug)]
pub struct UnsafeMidiInputPort {
    ptr: NonNull<ffi::AMidiInputPort>,
}

impl UnsafeMidiInputPort {
    /// Assumes ownership of `ptr`
    ///
    /// # Safety
    /// `ptr` must be a valid pointer to an Android [`ffi::AMidiInputPort`].
    pub unsafe fn from_ptr(ptr: NonNull<ffi::AMidiInputPort>) -> Self {
        Self { ptr }
    }

    pub fn ptr(&self) -> NonNull<ffi::AMidiInputPort> {
        self.ptr
    }

    /// Sends data to this port. Returns the number of bytes that were sent.
    ///
    /// # Arguments
    ///
    /// * `buffer`: The array of bytes containing the data to send.
    pub fn send(&self, buffer: &[u8]) -> Result<usize> {
        let num_bytes_sent =
            unsafe { ffi::AMidiInputPort_send(self.ptr.as_ptr(), buffer.as_ptr(), buffer.len()) };
        if num_bytes_sent >= 0 {
            Ok(num_bytes_sent as usize)
        } else {
            Err(MediaError::from_status(ffi::media_status_t(num_bytes_sent as c_int)).unwrap_err())
        }
    }

    /// Sends a message with a 'MIDI flush command code' to this port.
    ///
    /// This should cause a receiver to discard any pending MIDI data it may have accumulated and
    /// not processed.
    pub fn send_flush(&self) -> Result<()> {
        let result = unsafe { ffi::AMidiInputPort_sendFlush(self.ptr.as_ptr()) };
        MediaError::from_status(result)
    }

    /// Sends data to the specified input port with a timestamp. Sometimes it is convenient to send
    /// MIDI messages with a timestamp. By scheduling events in the future we can mask scheduling
    /// jitter. See the Java [Android MIDI docs] for more details.
    ///
    /// # Arguments
    ///
    /// * `buffer`: The array of bytes containing the data to send.
    /// * `timestamp`: The `CLOCK_MONOTONIC` time in nanoseconds to associate with the sent data.
    ///   This is consistent with the value returned by [`java.lang.System.nanoTime()`].
    ///
    /// [Android docs]: https://developer.android.com/reference/android/media/midi/package-summary#send_a_noteon
    /// [`java.lang.System.nanoTime()`]: https://developer.android.com/reference/java/lang/System#nanoTime()
    pub fn send_with_timestamp(&self, buffer: &[u8], timestamp: i64) -> Result<usize> {
        let num_bytes_sent = unsafe {
            ffi::AMidiInputPort_sendWithTimestamp(
                self.ptr.as_ptr(),
                buffer.as_ptr(),
                buffer.len(),
                timestamp,
            )
        };
        if num_bytes_sent >= 0 {
            Ok(num_bytes_sent as usize)
        } else {
            Err(MediaError::from_status(ffi::media_status_t(num_bytes_sent as c_int)).unwrap_err())
        }
    }
}

impl Drop for UnsafeMidiInputPort {
    fn drop(&mut self) {
        unsafe { ffi::AMidiInputPort_close(self.ptr.as_ptr()) };
    }
}

pub struct MidiInputPort {
    inner: UnsafeMidiInputPort,
    _guard: Arc<MidiDeviceGuard>,
}

impl fmt::Debug for MidiInputPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MidiInputPort")
            .field("inner", &self.inner.ptr)
            .finish()
    }
}

impl Deref for MidiInputPort {
    type Target = UnsafeMidiInputPort;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// SAFETY: a AMidi port is a mere holder of an atomic state, a pointer to the associated MIDI
// device, a binder, and a file descriptor, all of which are safe to be sent to another thread.
// https://cs.android.com/android/platform/superproject/main/+/main:frameworks/base/media/native/midi/amidi.cpp?q=symbol%3A%5CbAMIDI_Port%5Cb%20case%3Ayes
unsafe impl Send for MidiInputPort {}

// SAFETY: AMidiInputPort contains a file descriptor to a socket opened with the SOCK_SEQPACKET
// mode, which preserves message boundaries so the receiver of a message always reads the whole
// message.
// https://cs.android.com/android/platform/superproject/main/+/main:frameworks/base/media/native/midi/amidi.cpp?q=symbol%3A%5CbAMIDI_PACKET_SIZE%5Cb%20case%3Ayes
unsafe impl Sync for MidiInputPort {}

/// A thin wrapper over [`ffi::AMidiOutputPort`]. Please use [`MidiOutputPort`] if you need the
/// thread-safe version.
#[derive(Debug)]
pub struct UnsafeMidiOutputPort {
    ptr: NonNull<ffi::AMidiOutputPort>,
}

impl UnsafeMidiOutputPort {
    /// Assumes ownership of `ptr`
    ///
    /// # Safety
    /// `ptr` must be a valid pointer to an Android [`ffi::AMidiOutputPort`].
    pub unsafe fn from_ptr(ptr: NonNull<ffi::AMidiOutputPort>) -> Self {
        Self { ptr }
    }

    pub fn ptr(&self) -> NonNull<ffi::AMidiOutputPort> {
        self.ptr
    }

    /// Receives the next pending MIDI message.
    ///
    /// To retrieve all pending messages, the client should repeatedly call this method until it
    /// returns [`Ok(MidiOpcode::NoMessage)`].
    ///
    /// Note that this is a non-blocking call. If there are no Midi messages are available, the
    /// function returns [`Ok(MidiOpcode::NoMessage)`] immediately (for 0 messages received).
    ///
    /// When [`Ok(MidiOpcode::Data)`] is returned, the caller should limit `buffer` to
    /// [`MidiOpcode::Data::length`]. See [`MidiOpcode`] for more details.
    pub fn receive(&self, buffer: &mut [u8]) -> Result<MidiOpcode> {
        let mut opcode = 0i32;
        let mut timestamp = 0i64;
        let mut num_bytes_received = 0;
        let num_messages_received = unsafe {
            ffi::AMidiOutputPort_receive(
                self.ptr.as_ptr(),
                &mut opcode,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut num_bytes_received,
                &mut timestamp,
            )
        };

        match num_messages_received {
            r if r < 0 => {
                Err(MediaError::from_status(ffi::media_status_t(r as c_int)).unwrap_err())
            }
            0 => Ok(MidiOpcode::NoMessage),
            1 => match opcode as c_uint {
                ffi::AMIDI_OPCODE_DATA => Ok(MidiOpcode::Data {
                    length: num_bytes_received,
                    timestamp,
                }),
                ffi::AMIDI_OPCODE_FLUSH => Ok(MidiOpcode::Flush),
                _ => unreachable!("Unrecognized opcode {}", opcode),
            },
            r => unreachable!("Number of messages is positive integer {}", r),
        }
    }
}

impl Drop for UnsafeMidiOutputPort {
    fn drop(&mut self) {
        unsafe { ffi::AMidiOutputPort_close(self.ptr.as_ptr()) };
    }
}

pub struct MidiOutputPort {
    inner: UnsafeMidiOutputPort,
    _guard: Arc<MidiDeviceGuard>,
}

impl fmt::Debug for MidiOutputPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MidiOutputPort")
            .field("inner", &self.inner.ptr)
            .finish()
    }
}

impl Deref for MidiOutputPort {
    type Target = UnsafeMidiOutputPort;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// SAFETY: a AMidi port is a mere holder of an atomic state, a pointer to the associated MIDI
// device, a binder, and a file descriptor, all of which are safe to be sent to another thread.
// https://cs.android.com/android/platform/superproject/main/+/main:frameworks/base/media/native/midi/amidi.cpp?q=symbol%3A%5CbAMIDI_Port%5Cb%20case%3Ayes
unsafe impl Send for MidiOutputPort {}

// SAFETY: AMidiOutputPort is guarded by a atomic state ([`AMIDI_Port::state`]), which enables
// [`ffi::AMidiOutputPort_receive()`] to detect accesses from multiple threads and return error.
// https://cs.android.com/android/platform/superproject/main/+/main:frameworks/base/media/native/midi/amidi.cpp?q=symbol%3A%5CbMidiReceiver%3A%3Areceive%5Cb%20case%3Ayes
unsafe impl Sync for MidiOutputPort {}
