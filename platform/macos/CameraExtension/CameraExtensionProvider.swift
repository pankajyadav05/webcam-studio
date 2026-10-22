// The Webcam Studio virtual camera, modelled on Apple's "Camera Extension" Xcode template
// and WWDC22 session 10022. The extension offers one device with two streams:
//
//   * a source stream that Zoom, FaceTime, Safari and other apps read, and
//   * a sink stream that the Webcam Studio app writes finished frames into.
//
// Every frame the app writes is passed on to the source stream. When the app is not
// sending (or is not running), the source stream shows a "Webcam Studio is not running"
// picture instead, so meeting apps never see a frozen or black camera.
//
// Places where an exact Apple API name or signature should be checked if the compiler
// complains are marked `CHECK:`.

import CoreGraphics
import CoreMedia
import CoreMediaIO
import CoreText
import CoreVideo
import Foundation
import os

private let log = Logger(
    subsystem: "dev.pankajyadav.webcamstudio.CameraExtension", category: "camera")

// MARK: - Fixed values shared with the Webcam Studio app

/// These must match what the Rust side of the app expects. Do not change one side alone.
enum Contract {
    static let deviceName = "Webcam Studio Camera"
    static let manufacturer = "Webcam Studio"
    static let model = "Virtual Camera"
    static let sinkName = "Webcam Studio Camera Input"

    // Fixed IDs, so the app and meeting apps can find the camera again after a restart.
    static let deviceID = UUID(uuidString: "E0A5D920-6291-412E-80C9-99704E4E5218")!
    static let sourceStreamID = UUID(uuidString: "83148DC8-A158-4E84-9166-75E50BB46FB1")!
    static let sinkStreamID = UUID(uuidString: "01C4DA86-551A-49BD-882D-170FABF06336")!

    static let width: Int32 = 1280
    static let height: Int32 = 720
    static let framesPerSecond: Int32 = 30
    static let pixelFormat: OSType = kCVPixelFormatType_32BGRA

    /// After this long without a frame from the app, the placeholder takes over.
    static let appFrameTimeoutNanos: UInt64 = 1_000_000_000
}

// MARK: - Small helpers

private func hostTimeNow() -> CMTime {
    CMClockGetTime(CMClockGetHostTimeClock())
}

private func nanoseconds(_ time: CMTime) -> UInt64 {
    UInt64(time.seconds * Double(NSEC_PER_SEC))
}

/// Wraps a pixel buffer in a sample buffer stamped with the given host time.
private func makeSampleBuffer(
    from pixelBuffer: CVPixelBuffer, format: CMFormatDescription, time: CMTime
) -> CMSampleBuffer? {
    var timing = CMSampleTimingInfo(
        duration: CMTime(value: 1, timescale: Contract.framesPerSecond),
        presentationTimeStamp: time,
        decodeTimeStamp: .invalid)
    var sampleBuffer: CMSampleBuffer?
    let status = CMSampleBufferCreateForImageBuffer(
        allocator: kCFAllocatorDefault,
        imageBuffer: pixelBuffer,
        dataReady: true,
        makeDataReadyCallback: nil,
        refcon: nil,
        formatDescription: format,
        sampleTiming: &timing,
        sampleBufferOut: &sampleBuffer)
    return status == noErr ? sampleBuffer : nil
}

/// True if the buffer is exactly the size and pixel format we promised to apps.
private func matchesContract(_ pixelBuffer: CVPixelBuffer) -> Bool {
    CVPixelBufferGetWidth(pixelBuffer) == Int(Contract.width)
        && CVPixelBufferGetHeight(pixelBuffer) == Int(Contract.height)
        && CVPixelBufferGetPixelFormatType(pixelBuffer) == Contract.pixelFormat
}

// MARK: - Placeholder picture

/// Draws the "Webcam Studio is not running" picture once, into a buffer from a pool.
/// The same buffer is then sent over and over, so nothing is drawn per frame.
enum PlaceholderRenderer {
    static func makePixelBuffer() -> CVPixelBuffer? {
        // IOSurface-backed BGRA buffers, the same kind the app sends.
        let attributes: [CFString: Any] = [
            kCVPixelBufferWidthKey: Int(Contract.width),
            kCVPixelBufferHeightKey: Int(Contract.height),
            kCVPixelBufferPixelFormatTypeKey: Contract.pixelFormat,
            kCVPixelBufferIOSurfacePropertiesKey: [String: Any](),
        ]
        var pool: CVPixelBufferPool?
        CVPixelBufferPoolCreate(kCFAllocatorDefault, nil, attributes as CFDictionary, &pool)
        guard let pool else {
            log.error("Could not create the pixel buffer pool")
            return nil
        }
        var buffer: CVPixelBuffer?
        CVPixelBufferPoolCreatePixelBuffer(kCFAllocatorDefault, pool, &buffer)
        guard let buffer else {
            log.error("Could not get a pixel buffer from the pool")
            return nil
        }
        draw(into: buffer)
        return buffer
    }

    private static func draw(into buffer: CVPixelBuffer) {
        CVPixelBufferLockBaseAddress(buffer, [])
        defer { CVPixelBufferUnlockBaseAddress(buffer, []) }

        let width = CVPixelBufferGetWidth(buffer)
        let height = CVPixelBufferGetHeight(buffer)

        // BGRA in memory means 32-bit little-endian with the alpha channel first.
        let bitmapInfo =
            CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue
        guard
            let base = CVPixelBufferGetBaseAddress(buffer),
            let context = CGContext(
                data: base, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: CVPixelBufferGetBytesPerRow(buffer),
                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: bitmapInfo)
        else {
            log.error("Could not create a drawing context for the placeholder")
            return
        }

        // Dark graphite background (#0D0F12).
        let graphite = CGColor(red: 13.0 / 255, green: 15.0 / 255, blue: 18.0 / 255, alpha: 1)
        context.setFillColor(graphite)
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))

        // Small orange dot (#FF5B1F), centred a little above the middle.
        let orange = CGColor(red: 1.0, green: 91.0 / 255, blue: 31.0 / 255, alpha: 1)
        let radius: CGFloat = 14
        let centreX = CGFloat(width) / 2
        let centreY = CGFloat(height) / 2 + 30
        context.setFillColor(orange)
        context.fillEllipse(
            in: CGRect(
                x: centreX - radius, y: centreY - radius, width: radius * 2, height: radius * 2))

        // The message, centred under the dot, in a soft light grey.
        let text = "Webcam Studio is not running"
        let font = CTFontCreateWithName("Helvetica Neue" as CFString, 30, nil)
        let grey = CGColor(red: 0.78, green: 0.80, blue: 0.83, alpha: 1)
        let textAttributes: [CFString: Any] = [
            kCTFontAttributeName: font,
            kCTForegroundColorAttributeName: grey,
        ]
        guard
            let attributed = CFAttributedStringCreate(
                kCFAllocatorDefault, text as CFString, textAttributes as CFDictionary)
        else { return }
        let line = CTLineCreateWithAttributedString(attributed)
        let textWidth = CTLineGetTypographicBounds(line, nil, nil, nil)
        context.textPosition = CGPoint(
            x: (CGFloat(width) - CGFloat(textWidth)) / 2, y: centreY - radius - 56)
        CTLineDraw(line, context)
    }
}

// MARK: - Device

/// The one virtual camera. It owns both streams and decides, 30 times a second, whether
/// apps see the app's picture or the placeholder.
final class CameraDeviceSource: NSObject, CMIOExtensionDeviceSource {
    private(set) var device: CMIOExtensionDevice!

    let formatDescription: CMFormatDescription
    let streamFormat: CMIOExtensionStreamFormat

    private var sourceStream: CameraSourceStreamSource!
    private var sinkStream: CameraSinkStreamSource!

    /// The placeholder picture, drawn once.
    private let placeholder: CVPixelBuffer?

    // Everything below is only touched on `queue`.
    private let queue = DispatchQueue(
        label: "dev.pankajyadav.webcamstudio.CameraExtension.device", qos: .userInteractive)
    private var sourceReaders = 0  // apps currently reading the source stream
    private var sinkRunning = false  // the app has started the sink stream
    private var lastAppFrameNanos: UInt64 = 0  // 0 means no frame yet
    private var timer: DispatchSourceTimer?
    private var warnedAboutFormat = false

    override init() {
        // One format for both streams: 1280x720, 30 fps, BGRA.
        var videoFormat: CMFormatDescription?
        CMVideoFormatDescriptionCreate(
            allocator: kCFAllocatorDefault,
            codecType: Contract.pixelFormat,
            width: Contract.width,
            height: Contract.height,
            extensions: nil,
            formatDescriptionOut: &videoFormat)
        guard let videoFormat else { fatalError("Could not create the video format") }
        formatDescription = videoFormat

        let frameDuration = CMTime(value: 1, timescale: Contract.framesPerSecond)
        streamFormat = CMIOExtensionStreamFormat(
            formatDescription: videoFormat,
            maxFrameDuration: frameDuration,
            minFrameDuration: frameDuration,
            validFrameDurations: nil)

        placeholder = PlaceholderRenderer.makePixelBuffer()
        super.init()

        // CHECK: legacyDeviceID is what apps see as the device UID. OBS passes the UUID string.
        device = CMIOExtensionDevice(
            localizedName: Contract.deviceName,
            deviceID: Contract.deviceID,
            legacyDeviceID: Contract.deviceID.uuidString,
            source: self)

        sourceStream = CameraSourceStreamSource(owner: self)
        sinkStream = CameraSinkStreamSource(owner: self)
        do {
            try device.addStream(sourceStream.stream)
            try device.addStream(sinkStream.stream)
        } catch {
            fatalError("Could not add the streams to the device: \(error.localizedDescription)")
        }
    }

    var availableProperties: Set<CMIOExtensionProperty> { [.deviceModel] }

    func deviceProperties(forProperties properties: Set<CMIOExtensionProperty>) throws
        -> CMIOExtensionDeviceProperties
    {
        let result = CMIOExtensionDeviceProperties(dictionary: [:])
        if properties.contains(.deviceModel) {
            result.model = Contract.model
        }
        return result
    }

    func setDeviceProperties(_ deviceProperties: CMIOExtensionDeviceProperties) throws {
        // Nothing can be changed on this device.
    }

    // MARK: Called by the streams

    /// An app started reading the source stream. Start the placeholder timer if needed.
    func sourceDidStart() {
        queue.async {
            self.sourceReaders += 1
            if self.timer == nil { self.startTimer() }
            log.info("Source stream started, readers: \(self.sourceReaders)")
        }
    }

    /// An app stopped reading. When the last one leaves, stop the timer.
    func sourceDidStop() {
        queue.async {
            self.sourceReaders = max(0, self.sourceReaders - 1)
            if self.sourceReaders == 0 { self.stopTimer() }
            log.info("Source stream stopped, readers: \(self.sourceReaders)")
        }
    }

    func sinkDidStart() {
        queue.async {
            self.sinkRunning = true
            self.lastAppFrameNanos = 0
            log.info("Sink stream started")
        }
    }

    func sinkDidStop() {
        queue.async {
            self.sinkRunning = false
            log.info("Sink stream stopped")
        }
    }

    /// A frame arrived from the Webcam Studio app. Pass it on to the apps reading the camera.
    func receiveAppFrame(_ sampleBuffer: CMSampleBuffer) {
        queue.async {
            guard let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
            guard matchesContract(pixelBuffer) else {
                if !self.warnedAboutFormat {
                    self.warnedAboutFormat = true
                    log.error("Dropping app frames: they are not 1280x720 BGRA")
                }
                return
            }
            self.lastAppFrameNanos = nanoseconds(hostTimeNow())
            if self.sourceReaders > 0 {
                self.sendToSource(pixelBuffer)
            }
        }
    }

    // MARK: Placeholder timer (runs on `queue`)

    private func startTimer() {
        let newTimer = DispatchSource.makeTimerSource(queue: queue)
        let interval = 1.0 / Double(Contract.framesPerSecond)
        newTimer.schedule(deadline: .now(), repeating: interval, leeway: .milliseconds(1))
        newTimer.setEventHandler { [weak self] in self?.timerTick() }
        timer = newTimer
        newTimer.resume()
    }

    private func stopTimer() {
        timer?.cancel()
        timer = nil
    }

    /// Runs 30 times a second while an app reads the camera. Sends the placeholder only
    /// when the app has gone quiet, so real frames are never mixed with it.
    private func timerTick() {
        let now = nanoseconds(hostTimeNow())
        let age = now >= lastAppFrameNanos ? now - lastAppFrameNanos : 0
        let appIsLive =
            sinkRunning && lastAppFrameNanos != 0 && age < Contract.appFrameTimeoutNanos
        if !appIsLive, let placeholder {
            sendToSource(placeholder)
        }
    }

    // MARK: Sending (runs on `queue`)

    /// Stamps the picture with the current host time and hands it to the source stream.
    private func sendToSource(_ pixelBuffer: CVPixelBuffer) {
        let now = hostTimeNow()
        guard
            let sampleBuffer = makeSampleBuffer(
                from: pixelBuffer, format: formatDescription, time: now)
        else { return }
        sourceStream.stream.send(
            sampleBuffer, discontinuity: [], hostTimeInNanoseconds: nanoseconds(now))
    }
}

// MARK: - Source stream (what Zoom, FaceTime and Safari read)

final class CameraSourceStreamSource: NSObject, CMIOExtensionStreamSource {
    private(set) var stream: CMIOExtensionStream!
    private weak var owner: CameraDeviceSource?
    private let format: CMIOExtensionStreamFormat

    init(owner: CameraDeviceSource) {
        self.owner = owner
        self.format = owner.streamFormat
        super.init()
        stream = CMIOExtensionStream(
            localizedName: Contract.deviceName,
            streamID: Contract.sourceStreamID,
            direction: .source,
            clockType: .hostTime,
            source: self)
    }

    var formats: [CMIOExtensionStreamFormat] { [format] }
    var activeFormatIndex: Int = 0

    var availableProperties: Set<CMIOExtensionProperty> {
        [.streamActiveFormatIndex, .streamFrameDuration]
    }

    func streamProperties(forProperties properties: Set<CMIOExtensionProperty>) throws
        -> CMIOExtensionStreamProperties
    {
        let result = CMIOExtensionStreamProperties(dictionary: [:])
        if properties.contains(.streamActiveFormatIndex) {
            result.activeFormatIndex = 0
        }
        if properties.contains(.streamFrameDuration) {
            result.frameDuration = CMTime(value: 1, timescale: Contract.framesPerSecond)
        }
        return result
    }

    func setStreamProperties(_ streamProperties: CMIOExtensionStreamProperties) throws {
        // There is only one format, so there is nothing to change.
    }

    func authorizedToStartStream(for client: CMIOExtensionClient) -> Bool {
        true  // Any app may read the camera.
    }

    func startStream() throws { owner?.sourceDidStart() }
    func stopStream() throws { owner?.sourceDidStop() }
}

// MARK: - Sink stream (where the Webcam Studio app writes frames)

final class CameraSinkStreamSource: NSObject, CMIOExtensionStreamSource {
    private(set) var stream: CMIOExtensionStream!
    private weak var owner: CameraDeviceSource?
    private let format: CMIOExtensionStreamFormat

    // The sink callbacks come from different threads, so state is behind a lock.
    private let lock = NSLock()
    private var client: CMIOExtensionClient?
    private var running = false
    private var generation = 0  // bumps on every start/stop, so an old loop ends itself

    init(owner: CameraDeviceSource) {
        self.owner = owner
        self.format = owner.streamFormat
        super.init()
        stream = CMIOExtensionStream(
            localizedName: Contract.sinkName,
            streamID: Contract.sinkStreamID,
            direction: .sink,
            clockType: .hostTime,
            source: self)
    }

    var formats: [CMIOExtensionStreamFormat] { [format] }
    var activeFormatIndex: Int = 0

    var availableProperties: Set<CMIOExtensionProperty> {
        [
            .streamActiveFormatIndex, .streamFrameDuration,
            .streamSinkBufferQueueSize, .streamSinkBuffersRequiredForStartup,
        ]
    }

    func streamProperties(forProperties properties: Set<CMIOExtensionProperty>) throws
        -> CMIOExtensionStreamProperties
    {
        let result = CMIOExtensionStreamProperties(dictionary: [:])
        if properties.contains(.streamActiveFormatIndex) {
            result.activeFormatIndex = 0
        }
        if properties.contains(.streamFrameDuration) {
            result.frameDuration = CMTime(value: 1, timescale: Contract.framesPerSecond)
        }
        // CHECK: property names as in Apple's sink sample. A small queue keeps latency low.
        if properties.contains(.streamSinkBufferQueueSize) {
            result.sinkBufferQueueSize = 3
        }
        if properties.contains(.streamSinkBuffersRequiredForStartup) {
            result.sinkBuffersRequiredForStartup = 1
        }
        return result
    }

    func setStreamProperties(_ streamProperties: CMIOExtensionStreamProperties) throws {}

    func authorizedToStartStream(for client: CMIOExtensionClient) -> Bool {
        // Remember who is writing, so startStream() knows whom to read frames from.
        // CHECK: signingID exists on CMIOExtensionClient. For v0 any app may write; a later
        // version should compare it with the Webcam Studio app's signing identity.
        log.info("Sink writer: \(String(describing: client.signingID))")
        lock.lock()
        self.client = client
        lock.unlock()
        return true
    }

    func startStream() throws {
        lock.lock()
        running = true
        generation += 1
        let myGeneration = generation
        let writer = client
        lock.unlock()

        owner?.sinkDidStart()
        guard let writer else {
            log.error("Sink started without a known writer")
            return
        }
        consumeNextFrame(from: writer, generation: myGeneration)
    }

    func stopStream() throws {
        lock.lock()
        running = false
        generation += 1
        lock.unlock()
        owner?.sinkDidStop()
    }

    private func isCurrent(_ id: Int) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return running && generation == id
    }

    /// Takes the next frame the app queued, passes it on, then asks for the one after.
    private func consumeNextFrame(from writer: CMIOExtensionClient, generation id: Int) {
        guard isCurrent(id) else { return }
        // CHECK: completion handler arguments (buffer, sequence number, discontinuity,
        // more buffers waiting, error) as in Apple's sink sample.
        stream.consumeSampleBuffer(from: writer) {
            [weak self] sampleBuffer, sequenceNumber, _, _, error in
            guard let self, self.isCurrent(id) else { return }

            if let sampleBuffer {
                self.owner?.receiveAppFrame(sampleBuffer)
                // Tell CoreMediaIO the frame has been used, so the app can reuse its buffer.
                // CHECK: CMIOExtensionScheduledOutput initializer labels.
                let output = CMIOExtensionScheduledOutput(
                    sequenceNumber: sequenceNumber,
                    hostTimeInNanoseconds: nanoseconds(hostTimeNow()))
                self.stream.notifyScheduledOutputChanged(output)
                self.consumeNextFrame(from: writer, generation: id)
            } else {
                if let error {
                    log.error("Sink read failed: \(error.localizedDescription)")
                }
                // Nothing to read right now. Wait briefly so this never becomes a busy loop.
                DispatchQueue.global(qos: .userInteractive).asyncAfter(
                    deadline: .now() + .milliseconds(5)
                ) {
                    self.consumeNextFrame(from: writer, generation: id)
                }
            }
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
        deviceSource = CameraDeviceSource()
        do {
            try provider.addDevice(deviceSource.device)
        } catch {
            fatalError("Could not add the device: \(error.localizedDescription)")
        }
    }

    func connect(to client: CMIOExtensionClient) throws {}
    func disconnect(from client: CMIOExtensionClient) {}

    var availableProperties: Set<CMIOExtensionProperty> { [.providerManufacturer] }

    func providerProperties(forProperties properties: Set<CMIOExtensionProperty>) throws
        -> CMIOExtensionProviderProperties
    {
        let result = CMIOExtensionProviderProperties(dictionary: [:])
        if properties.contains(.providerManufacturer) {
            result.manufacturer = Contract.manufacturer
        }
        return result
    }

    func setProviderProperties(_ providerProperties: CMIOExtensionProviderProperties) throws {}
}
