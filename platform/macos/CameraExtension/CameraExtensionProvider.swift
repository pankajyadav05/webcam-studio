// Skeleton of the Webcam Studio virtual camera, modelled on Apple's Camera Extension
// template in Xcode. Check every API detail against Apple's CoreMediaIO documentation
// before relying on it; places that need a check are marked TODO.

import CoreMedia
import CoreMediaIO
import Foundation
import IOKit.audio
import os.log

private let log = Logger(subsystem: "com.example.webcamstudio.CameraExtension", category: "provider")

/// Output size and frame rate. TODO: let the app choose these instead of fixing them.
private let outputWidth: Int32 = 1280
private let outputHeight: Int32 = 720
private let outputFrameRate: Double = 30

// MARK: - Device

/// One virtual camera device with a source stream (what apps read) and a sink stream
/// (where the Webcam Studio app writes frames).
final class CameraDeviceSource: NSObject, CMIOExtensionDeviceSource {
    private(set) var device: CMIOExtensionDevice!
    private var sourceStream: CameraSourceStream!
    private var sinkStream: CameraSinkStream!

    init(localizedName: String) {
        super.init()
        device = CMIOExtensionDevice(
            localizedName: localizedName,
            deviceID: UUID(),  // TODO: use a fixed UUID so apps remember the camera.
            legacyDeviceID: nil,
            source: self
        )

        let format = CameraSourceStream.makeFormat()
        sourceStream = CameraSourceStream(localizedName: "\(localizedName) Video", format: format)
        sinkStream = CameraSinkStream(localizedName: "\(localizedName) Input", format: format)
        sinkStream.onFrame = { [weak self] sampleBuffer in
            self?.sourceStream.forward(sampleBuffer)
        }

        do {
            try device.addStream(sourceStream.stream)
            try device.addStream(sinkStream.stream)
        } catch {
            fatalError("Could not add streams to the device: \(error.localizedDescription)")
        }
    }

    var availableProperties: Set<CMIOExtensionProperty> {
        [.deviceTransportType, .deviceModel]
    }

    func deviceProperties(forProperties properties: Set<CMIOExtensionProperty>) throws
        -> CMIOExtensionDeviceProperties
    {
        let state = CMIOExtensionDeviceProperties(dictionary: [:])
        if properties.contains(.deviceTransportType) {
            // TODO: confirm the right transport type constant for a virtual device.
            state.transportType = kIOAudioDeviceTransportTypeVirtual
        }
        if properties.contains(.deviceModel) {
            state.model = "Webcam Studio Virtual Camera"
        }
        return state
    }

    func setDeviceProperties(_ deviceProperties: CMIOExtensionDeviceProperties) throws {
        // No settable device properties.
    }
}

// MARK: - Source stream (what Zoom, Teams, Safari read)

final class CameraSourceStream: NSObject, CMIOExtensionStreamSource {
    private(set) var stream: CMIOExtensionStream!
    private let streamFormat: CMIOExtensionStreamFormat
    private var isStreaming = false

    init(localizedName: String, format: CMIOExtensionStreamFormat) {
        streamFormat = format
        super.init()
        stream = CMIOExtensionStream(
            localizedName: localizedName,
            streamID: UUID(),  // TODO: use a fixed UUID.
            direction: .source,
            clockType: .hostTime,
            source: self
        )
    }

    /// Builds the single BGRA format both streams use.
    static func makeFormat() -> CMIOExtensionStreamFormat {
        var description: CMFormatDescription?
        CMVideoFormatDescriptionCreate(
            allocator: kCFAllocatorDefault,
            codecType: kCVPixelFormatType_32BGRA,
            width: outputWidth,
            height: outputHeight,
            extensions: nil,
            formatDescriptionOut: &description
        )
        let frameDuration = CMTime(value: 1, timescale: CMTimeScale(outputFrameRate))
        return CMIOExtensionStreamFormat(
            formatDescription: description!,
            maxFrameDuration: frameDuration,
            minFrameDuration: frameDuration,
            validFrameDurations: nil
        )
    }

    var formats: [CMIOExtensionStreamFormat] { [streamFormat] }
    var activeFormatIndex: Int = 0

    var availableProperties: Set<CMIOExtensionProperty> {
        [.streamActiveFormatIndex, .streamFrameDuration]
    }

    func streamProperties(forProperties properties: Set<CMIOExtensionProperty>) throws
        -> CMIOExtensionStreamProperties
    {
        let state = CMIOExtensionStreamProperties(dictionary: [:])
        if properties.contains(.streamActiveFormatIndex) {
            state.activeFormatIndex = 0
        }
        if properties.contains(.streamFrameDuration) {
            state.frameDuration = CMTime(value: 1, timescale: CMTimeScale(outputFrameRate))
        }
        return state
    }

    func setStreamProperties(_ streamProperties: CMIOExtensionStreamProperties) throws {
        // Only one format, nothing to change.
    }

    func authorizedToStartStream(for client: CMIOExtensionClient) -> Bool {
        // TODO: decide whether to restrict which apps may read the camera.
        true
    }

    func startStream() throws { isStreaming = true }
    func stopStream() throws { isStreaming = false }

    /// Sends a frame that arrived on the sink stream to the apps reading this stream.
    func forward(_ sampleBuffer: CMSampleBuffer) {
        guard isStreaming else { return }
        // TODO: re-stamp the buffer with a timing that matches the host clock, and
        // send a "no signal" placeholder frame when the app stops sending.
        stream.send(
            sampleBuffer,
            discontinuity: [],
            hostTimeInNanoseconds: UInt64(
                sampleBuffer.presentationTimeStamp.seconds * Double(NSEC_PER_SEC))
        )
    }
}

// MARK: - Sink stream (where the Webcam Studio app writes frames)

final class CameraSinkStream: NSObject, CMIOExtensionStreamSource {
    private(set) var stream: CMIOExtensionStream!
    private let streamFormat: CMIOExtensionStreamFormat
    private var client: CMIOExtensionClient?

    /// Called for every frame the app delivers.
    var onFrame: ((CMSampleBuffer) -> Void)?

    init(localizedName: String, format: CMIOExtensionStreamFormat) {
        streamFormat = format
        super.init()
        stream = CMIOExtensionStream(
            localizedName: localizedName,
            streamID: UUID(),  // TODO: use a fixed UUID the app can look up.
            direction: .sink,
            clockType: .hostTime,
            source: self
        )
    }

    var formats: [CMIOExtensionStreamFormat] { [streamFormat] }
    var activeFormatIndex: Int = 0
    var availableProperties: Set<CMIOExtensionProperty> { [.streamSinkBufferQueueSize] }

    func streamProperties(forProperties properties: Set<CMIOExtensionProperty>) throws
        -> CMIOExtensionStreamProperties
    {
        let state = CMIOExtensionStreamProperties(dictionary: [:])
        // TODO: report a buffer queue size (a few frames) so the app knows how deep it may queue.
        return state
    }

    func setStreamProperties(_ streamProperties: CMIOExtensionStreamProperties) throws {}

    func authorizedToStartStream(for client: CMIOExtensionClient) -> Bool {
        // TODO: only accept the Webcam Studio app, for example by checking the client's
        // code signature or bundle identifier, so other apps cannot inject frames.
        self.client = client
        return true
    }

    func startStream() throws {
        pullNextFrame()
    }

    func stopStream() throws {
        client = nil
    }

    /// Asks CoreMediaIO for the next frame the app queued, and loops until stopped.
    private func pullNextFrame() {
        guard let client else { return }
        stream.consumeSampleBuffer(from: client) { [weak self] sampleBuffer, _, _, _, error in
            guard let self else { return }
            if let sampleBuffer {
                // TODO: frames from the app arrive here, backed by an IOSurface. Check the
                // pixel format and size against the active format before forwarding.
                self.onFrame?(sampleBuffer)
                // TODO: tell CoreMediaIO the frame was output, using the stream's
                // notifyScheduledOutputChanged call, so the app can reuse the buffer.
            } else if let error {
                log.error("Sink stream error: \(error.localizedDescription)")
            }
            self.pullNextFrame()
        }
    }
}

// MARK: - Provider

final class CameraProviderSource: NSObject, CMIOExtensionProviderSource {
    private(set) var provider: CMIOExtensionProvider!
    private var deviceSource: CameraDeviceSource!

    init(clientQueue: DispatchQueue?) {
        super.init()
        provider = CMIOExtensionProvider(source: self, clientQueue: clientQueue)
        deviceSource = CameraDeviceSource(localizedName: "Webcam Studio")
        do {
            try provider.addDevice(deviceSource.device)
        } catch {
            fatalError("Could not add the device: \(error.localizedDescription)")
        }
    }

    func connect(to client: CMIOExtensionClient) throws {
        // TODO: remember connected clients if per-app behaviour is needed.
    }

    func disconnect(from client: CMIOExtensionClient) {}

    var availableProperties: Set<CMIOExtensionProperty> { [.providerManufacturer] }

    func providerProperties(forProperties properties: Set<CMIOExtensionProperty>) throws
        -> CMIOExtensionProviderProperties
    {
        let state = CMIOExtensionProviderProperties(dictionary: [:])
        if properties.contains(.providerManufacturer) {
            state.manufacturer = "Webcam Studio"
        }
        return state
    }

    func setProviderProperties(_ providerProperties: CMIOExtensionProviderProperties) throws {}
}
