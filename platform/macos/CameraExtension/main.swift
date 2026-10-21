// Entry point of the camera extension process.
//
// macOS starts this process when an app first asks for the camera. It creates the
// provider and then hands control to the CoreMediaIO service loop.

import CoreMediaIO
import Foundation

let providerSource = CameraProviderSource(clientQueue: nil)
CMIOExtensionProvider.startService(provider: providerSource.provider)

// Keep the process alive so CoreMediaIO can call into the provider.
CFRunLoopRun()
