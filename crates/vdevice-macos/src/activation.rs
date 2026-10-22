//! Installing and removing the Camera Extension with a system extension request.

use std::cell::RefCell;

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, Message, define_class, msg_send};
use objc2_foundation::{NSError, NSObject, NSObjectProtocol, NSString};
use objc2_system_extensions::{
    OSSystemExtensionErrorCode, OSSystemExtensionManager, OSSystemExtensionProperties,
    OSSystemExtensionReplacementAction, OSSystemExtensionRequest, OSSystemExtensionRequestDelegate,
    OSSystemExtensionRequestResult,
};
use studio_core::error::{Result, StudioError};

use crate::{ActivationOutcome, EXTENSION_BUNDLE_ID};

type ActivationCallback = Box<dyn Fn(Result<ActivationOutcome>) + Send + Sync>;
type DeactivationCallback = Box<dyn Fn(Result<()>) + Send + Sync>;

/// What to tell the caller. A delegate does exactly one of the two jobs.
enum Callback {
    Activate(ActivationCallback),
    Deactivate(DeactivationCallback),
}

impl Callback {
    /// Reports a final result that is not an error.
    fn finish(&self, result: OSSystemExtensionRequestResult) {
        match self {
            Callback::Activate(on_update) => {
                let outcome = if result == OSSystemExtensionRequestResult::WillCompleteAfterReboot {
                    ActivationOutcome::WillCompleteAfterReboot
                } else {
                    ActivationOutcome::Activated
                };
                on_update(Ok(outcome));
            }
            Callback::Deactivate(on_done) => on_done(Ok(())),
        }
    }

    fn fail(&self, error: StudioError) {
        match self {
            Callback::Activate(on_update) => on_update(Err(error)),
            Callback::Deactivate(on_done) => on_done(Err(error)),
        }
    }

    fn needs_approval(&self) {
        // Removing an extension never asks for approval, so only activation reports this.
        if let Callback::Activate(on_update) = self {
            on_update(Ok(ActivationOutcome::NeedsUserApproval));
        }
    }
}

struct DelegateIvars {
    callback: Callback,
}

define_class!(
    // SAFETY:
    // - The superclass NSObject has no subclassing requirements.
    // - `RequestDelegate` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "WebcamStudioExtensionRequestDelegate"]
    #[ivars = DelegateIvars]
    struct RequestDelegate;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for RequestDelegate {}

    // SAFETY: `OSSystemExtensionRequestDelegate` has no safety requirements beyond the
    // method signatures, which match the protocol.
    unsafe impl OSSystemExtensionRequestDelegate for RequestDelegate {
        #[unsafe(method(request:actionForReplacingExtension:withExtension:))]
        fn request_action_for_replacing_extension(
            &self,
            _request: &OSSystemExtensionRequest,
            _existing: &OSSystemExtensionProperties,
            _extension: &OSSystemExtensionProperties,
        ) -> OSSystemExtensionReplacementAction {
            // A newer or older build of our own extension is already there: replace it.
            OSSystemExtensionReplacementAction::Replace
        }

        #[unsafe(method(requestNeedsUserApproval:))]
        fn request_needs_user_approval(&self, _request: &OSSystemExtensionRequest) {
            self.ivars().callback.needs_approval();
        }

        #[unsafe(method(request:didFinishWithResult:))]
        fn request_did_finish_with_result(
            &self,
            _request: &OSSystemExtensionRequest,
            result: OSSystemExtensionRequestResult,
        ) {
            // Keep ourselves alive until the callback has returned, then let go.
            let _keep_alive = self.retain();
            self.ivars().callback.finish(result);
            forget(self);
        }

        #[unsafe(method(request:didFailWithError:))]
        fn request_did_fail_with_error(
            &self,
            _request: &OSSystemExtensionRequest,
            error: &NSError,
        ) {
            let _keep_alive = self.retain();
            self.ivars().callback.fail(describe_error(error));
            forget(self);
        }
    }
);

impl RequestDelegate {
    fn new(callback: Callback) -> Retained<Self> {
        let this = Self::alloc().set_ivars(DelegateIvars { callback });
        // SAFETY: The signature of `NSObject`'s `init` method is correct.
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    /// Delegates of requests that have not finished yet. macOS does not keep its delegate
    /// alive, so we must. Requests are submitted on the main queue and answered there, so
    /// only the main thread ever touches this list.
    static PENDING: RefCell<Vec<Retained<RequestDelegate>>> = const { RefCell::new(Vec::new()) };
}

/// Stops keeping `delegate` alive.
fn forget(delegate: &RequestDelegate) {
    PENDING.with_borrow_mut(|pending| {
        pending.retain(|kept| !std::ptr::eq(&**kept, delegate));
    });
}

/// Turns a system extension error into a message a person can act on.
fn describe_error(error: &NSError) -> StudioError {
    let code = OSSystemExtensionErrorCode(error.code());
    let hint = if code == OSSystemExtensionErrorCode::UnsupportedParentBundleLocation {
        Some("Move Webcam Studio to the Applications folder and open it from there.")
    } else if code == OSSystemExtensionErrorCode::MissingEntitlement {
        Some("This build of Webcam Studio is not signed to install camera extensions.")
    } else if code == OSSystemExtensionErrorCode::ForbiddenBySystemPolicy {
        Some("A security policy on this Mac blocks the camera extension.")
    } else if code == OSSystemExtensionErrorCode::RequestCanceled {
        Some("The request was cancelled.")
    } else {
        None
    };
    let message = error.localizedDescription().to_string();
    StudioError::Backend(match hint {
        Some(hint) => format!("{message} {hint}"),
        None => message,
    })
}

/// Creates the request on the main queue and sends it to macOS.
fn submit(callback: Callback, activate: bool) {
    // The delegate and the request are not thread-safe objects, so they are created on the
    // main queue, where the answers come back too. Only the callback crosses threads.
    DispatchQueue::main().exec_async(move || {
        let queue = DispatchQueue::main();
        let identifier = NSString::from_str(EXTENSION_BUNDLE_ID);
        // SAFETY: `identifier` and `queue` are valid objects.
        let request = unsafe {
            if activate {
                OSSystemExtensionRequest::activationRequestForExtension_queue(&identifier, queue)
            } else {
                OSSystemExtensionRequest::deactivationRequestForExtension_queue(&identifier, queue)
            }
        };
        let delegate = RequestDelegate::new(callback);
        // SAFETY: The delegate implements the protocol and stays alive in `PENDING` until
        // the request reaches a final state.
        unsafe { request.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        PENDING.with_borrow_mut(|pending| pending.push(delegate));
        // SAFETY: `sharedManager` and `submitRequest:` may be called from the main queue.
        unsafe { OSSystemExtensionManager::sharedManager().submitRequest(&request) };
    });
}

/// Asks macOS to install (or update) the Camera Extension.
///
/// `on_update` runs on the main thread. It can be called more than once:
///
/// - `Ok(NeedsUserApproval)`: the user must allow the extension in System Settings. This
///   is not final. When they decide, `on_update` is called again.
/// - `Ok(Activated)` or `Ok(WillCompleteAfterReboot)`: final success.
/// - `Err(..)`: final failure, for example when the app is not in `/Applications`.
///
/// The app must be signed with a Developer ID and have the system extension entitlement.
pub fn request_activation(on_update: Box<dyn Fn(Result<ActivationOutcome>) + Send + Sync>) {
    submit(Callback::Activate(on_update), true);
}

/// Asks macOS to remove the Camera Extension.
///
/// `on_done` runs once on the main thread, with `Ok(())` when the extension is gone.
pub fn request_deactivation(on_done: Box<dyn Fn(Result<()>) + Send + Sync>) {
    submit(Callback::Deactivate(on_done), false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn callback_maps_results() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let callback = Callback::Activate(Box::new(move |result| {
            sink.lock().unwrap().push(result.map_err(|e| e.to_string()));
        }));
        callback.needs_approval();
        callback.finish(OSSystemExtensionRequestResult::Completed);
        callback.finish(OSSystemExtensionRequestResult::WillCompleteAfterReboot);
        callback.fail(StudioError::Backend("nope".to_owned()));

        let seen = seen.lock().unwrap();
        assert_eq!(seen[0], Ok(ActivationOutcome::NeedsUserApproval));
        assert_eq!(seen[1], Ok(ActivationOutcome::Activated));
        assert_eq!(seen[2], Ok(ActivationOutcome::WillCompleteAfterReboot));
        assert!(seen[3].is_err());
    }

    #[test]
    fn deactivation_ignores_approval() {
        let calls = Arc::new(Mutex::new(0));
        let counter = Arc::clone(&calls);
        let callback = Callback::Deactivate(Box::new(move |_| *counter.lock().unwrap() += 1));
        callback.needs_approval();
        assert_eq!(*calls.lock().unwrap(), 0);
        callback.finish(OSSystemExtensionRequestResult::Completed);
        assert_eq!(*calls.lock().unwrap(), 1);
    }
}
