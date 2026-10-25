// Dev-only native WebKit probe. Run while server.mjs is listening:
// swift apps/desktop/dev/engine-demo/verify-webkit.swift
import Cocoa
import WebKit
let qualityMode=CommandLine.arguments.contains("--quality")
let mattingMode=CommandLine.arguments.contains("--mediapipe") ? "mediapipe" : CommandLine.arguments.contains("--rvm") ? "rvm" : "hybrid"
let testKind=CommandLine.arguments.first(where:{$0.hasPrefix("--case=")})?.replacingOccurrences(of:"--case=",with:"") ?? "normal"
var probeFailed=false
var nativeRequests=[String]()
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let window = NSWindow(contentRect: NSRect(x:0,y:0,width:1320,height:980), styleMask:[.titled], backing:.buffered, defer:false)
final class Assets: NSObject, WKURLSchemeHandler {
  func webView(_ webView: WKWebView, start task: WKURLSchemeTask) {
    let path=task.request.url!.path
    if qualityMode {
      do {
        let data=try Data(contentsOf:URL(string:"http://127.0.0.1:4181"+path)!)
        let mime=path.hasSuffix(".html") ? "text/html" : path.hasSuffix(".js") || path.hasSuffix(".mjs") ? "text/javascript" : path.hasSuffix(".wasm") ? "application/wasm" : path.hasSuffix(".json") ? "application/json" : path.hasSuffix(".png") ? "image/png" : "application/octet-stream"
        task.didReceive(HTTPURLResponse(url:task.request.url!,statusCode:200,httpVersion:"HTTP/1.1",headerFields:["Content-Type":mime])!);task.didReceive(data);task.didFinish()
      }catch{task.didFailWithError(error)}
      return
    }
    nativeRequests.append(task.request.url!.absoluteString)
    let root=FileManager.default.currentDirectoryPath
    var file=path=="/fixture/portrait.jpg" ? "/tmp/engine-portrait.jpg" : root + (path.hasPrefix("/engine/") ? "/apps/desktop/ui"+path : "/apps/desktop/dev/engine-demo"+(path=="/" ? "/index.html" : path))
    if CommandLine.arguments.contains("--baseline") && path == "/engine/studio-engine.js" {file="/tmp/engine-shots/v2/studio-engine-before.js"}
    if path == "/fixture/wave.mp4" {file="/tmp/matting-bakeoff/footage/wave.mp4"}
    let ext=URL(fileURLWithPath:file).pathExtension
    let mime=["html":"text/html","js":"text/javascript","mjs":"text/javascript","css":"text/css","wasm":"application/wasm","jpg":"image/jpeg","mp4":"video/mp4"][ext] ?? "application/octet-stream"
    do {
      var data=try Data(contentsOf:URL(fileURLWithPath:file))
      if path == "/engine/rvm-worker.js", var body=String(data:data,encoding:.utf8) {
        if testKind == "rvm-fail" {body="throw new Error('Forced RVM failure');"}
        data=Data(body.utf8)
      }
      if path == "/engine/studio-engine.js", var body=String(data:data,encoding:.utf8) {
        if testKind == "gate-fail" {body=body.replacingOccurrences(of:"gateMode = !!data.gate;",with:"gateMode = !!data.gate;if(gateMode)setTimeout(()=>{throw new Error('Forced gate failure');},0);")}
        data=Data(body.utf8)
      }
      if CommandLine.arguments.contains("--wasm") && path == "/engine/rvm-worker.js", let text=String(data:data,encoding:.utf8) {var body=text.replacingOccurrences(of:"await init(data.preferWasm)",with:"await init(true)");if CommandLine.arguments.contains("--keep-slow") {body=body.replacingOccurrences(of:"policy.sample(result.segmentMs)",with:"null")};data=Data(body.utf8)}
      let response=HTTPURLResponse(url:task.request.url!,statusCode:200,httpVersion:"HTTP/1.1",headerFields:["Content-Type":mime,"Access-Control-Allow-Origin":"*"])!
      task.didReceive(response);task.didReceive(data);task.didFinish()
    } catch {task.didFailWithError(error)}
  }
  func webView(_ webView: WKWebView, stop task: WKURLSchemeTask) {}
}
let v3=CommandLine.arguments.contains("--v3")
let v2=CommandLine.arguments.contains("--v2")
let chair=CommandLine.arguments.contains("--chair")
let replacing=CommandLine.arguments.contains("--replace")
let before=CommandLine.arguments.contains("--baseline")
let prefix="/tmp/engine-shots/v4/webkit-\(mattingMode)-\(CommandLine.arguments.contains("--wasm") ? "wasm" : "webgpu")-\(replacing ? "replace" : "blur")-\(testKind)"
final class Artifact: NSObject, WKScriptMessageHandler {
 func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
  guard let body=message.body as? [String:Any],let name=body["name"] as? String,let text=body["data"] as? String,!name.contains(".."),!name.hasPrefix("/") else{return}
  let file=URL(fileURLWithPath:"/tmp/engine-shots/v4/native-quality/"+name)
  try? FileManager.default.createDirectory(at:file.deletingLastPathComponent(),withIntermediateDirectories:true)
  let data=(body["json"] as? Bool == true) ? text.data(using:.utf8) : Data(base64Encoded:text)
  try? data?.write(to:file)
 }
}
let config=WKWebViewConfiguration()
config.userContentController.add(Artifact(),name:"artifact")
let instrumentation="""
window.transferCallbackTimes=[];window.sourceCallbackTimes=[];window.phase="load";window.overBudget=[];window.frameCallbackTimes=[];window.maskCallbackTimes=[];window.probeSamples=[];
const raf=requestAnimationFrame;window.requestAnimationFrame=cb=>raf.call(window,t=>{const start=performance.now();try{cb(t);}finally{const elapsed=performance.now()-start;if(phase==="steady")frameCallbackTimes.push(elapsed);if(elapsed>16)overBudget.push({start,duration:elapsed,phase,stats:window.demo?.engine.stats()});}});
const vf=HTMLVideoElement.prototype.requestVideoFrameCallback;if(vf)HTMLVideoElement.prototype.requestVideoFrameCallback=function(cb){return vf.call(this,(t,m)=>{const start=performance.now();try{cb(t,m);}finally{const duration=performance.now()-start;if(phase==="steady")sourceCallbackTimes.push(duration);if(duration>16)overBudget.push({start,duration,phase,kind:'source',stats:window.demo?.engine.stats()});}});};
const Native=Worker;window.Worker=class extends Native{postMessage(...args){const start=performance.now();try{return super.postMessage(...args);}finally{const duration=performance.now()-start;if(phase==="steady")transferCallbackTimes.push(duration);if(duration>16)overBudget.push({start,duration,phase,kind:'transfer',stats:window.demo?.engine.stats()});}}set onmessage(cb){super.onmessage=cb&&(e=>{const start=performance.now();try{cb(e);}finally{if(phase==="steady")maskCallbackTimes.push(performance.now()-start);}});}};
window.probeSummary=a=>{a.sort((x,y)=>x-y);return {count:a.length,p95:a[Math.floor(a.length*.95)],max:a.at(-1)};};
setInterval(()=>{if(window.demo){const s=demo.engine.stats();if(s.inferMs>0&&phase==="steady")probeSamples.push(s);}},1000);
"""
config.userContentController.addUserScript(WKUserScript(source:instrumentation,injectionTime:.atDocumentStart,forMainFrameOnly:true))
let assetMode=CommandLine.arguments.contains("--asset")
if assetMode {config.setURLSchemeHandler(Assets(),forURLScheme:"tauri")}
let web = WKWebView(frame:window.contentView!.bounds,configuration:config)
window.contentView = web
window.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps:true)
final class Delegate: NSObject, WKNavigationDelegate {
  func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
    if qualityMode {pollQuality(webView);return}
    DispatchQueue.main.asyncAfter(deadline:.now()+3) {
      webView.evaluateJavaScript("window.demo ? (demo.engine.setMatting('\(mattingMode)'),demo.mode('\(replacing ? "replace" : "blur")'), 'started') : 'missing'") { result, error in
        print("START", result ?? "nil", error?.localizedDescription ?? "")
      }
    }
    DispatchQueue.main.asyncAfter(deadline:.now()+10) {
      webView.evaluateJavaScript("window.phase='steady';window.probeSamples=[];window.frameCallbackTimes=[];window.maskCallbackTimes=[];window.sourceCallbackTimes=[]") {_,_ in}
    }
    DispatchQueue.main.asyncAfter(deadline:.now()+(testKind == "normal" ? 40 : 20)) {
      webView.evaluateJavaScript("JSON.stringify({samples:probeSamples,overBudget,transferCallbacks:probeSummary(transferCallbackTimes),sourceCallbacks:probeSummary(sourceCallbackTimes),frameCallbacks:probeSummary(frameCallbackTimes),maskCallbacks:probeSummary(maskCallbackTimes),longTasks:demo.longTasks,stats:demo.engine.stats(),statuses:demo.statuses,violations:demo.violations,requests:performance.getEntriesByType('resource').map(x=>x.name),video:{ready:demo.video.readyState,width:demo.video.videoWidth,height:demo.video.videoHeight,paused:demo.video.paused,time:demo.video.currentTime},hidden:document.hidden,ua:navigator.userAgent})") { result,error in
        print("WEBKIT",result ?? "nil",error?.localizedDescription ?? "")
        if let text = result as? String {
          if let data=text.data(using:.utf8),var report=(try? JSONSerialization.jsonObject(with:data)) as? [String:Any] {
            report["nativeRequests"]=nativeRequests
            if let saved=try? JSONSerialization.data(withJSONObject:report,options:[.prettyPrinted,.sortedKeys]) {try? saved.write(to:URL(fileURLWithPath:prefix+"-report.json"))}
          }
          if v2, let data=text.data(using:.utf8), let report=(try? JSONSerialization.jsonObject(with:data)) as? [String:Any] {
            let samples=Array(((report["samples"] as? [[String:Any]]) ?? []).suffix(20))
            let frames=report["frameCallbacks"] as? [String:Any] ?? [:]
            let violations=report["violations"] as? [[String:Any]] ?? []
            probeFailed=samples.count<10 || samples.contains {($0["fps"] as? Double ?? 0)<24} || (frames["max"] as? Double ?? 100)>16 || !violations.isEmpty
            print(probeFailed ? "BUDGET FAIL" : "BUDGET PASS")
          }
        } else {probeFailed=true}
        webView.takeSnapshot(with:nil) { image,error in
          if let tiff=image?.tiffRepresentation, let bitmap=NSBitmapImageRep(data:tiff), let png=bitmap.representation(using:.png,properties:[:]) {try? png.write(to:URL(fileURLWithPath:prefix+"-\(replacing ? "replace" : "blur").png"))}
          webView.evaluateJavaScript("demo.engine.dispose()") {_,_ in if probeFailed {exit(1)}; app.terminate(nil)}
        }
      }
    }
  }
}
func pollQuality(_ view: WKWebView) {
 view.evaluateJavaScript("window.quality?.done ? JSON.stringify({errors:quality.errors,violations:quality.violations,frames:quality.metrics.length}) : null") {result,error in
  if let text=result as? String {print("QUALITY",text);app.terminate(nil)}
  else {DispatchQueue.main.asyncAfter(deadline:.now()+1){pollQuality(view)}}
 }
}
let delegate = Delegate(); web.navigationDelegate = delegate
web.load(URLRequest(url:URL(string:qualityMode ? "tauri://localhost/hybrid-quality.html" : assetMode ? "tauri://localhost/?\(CommandLine.arguments.contains("--clip") ? "clip=wave&flat" : "portrait")\(chair ? "&chair" : "")" : "http://127.0.0.1:4178/?portrait\(chair ? "&chair" : "")")!))
DispatchQueue.main.asyncAfter(deadline:.now()+(qualityMode ? 300 : 55)) {print("WEBKIT timeout");exit(1)}
app.run()
if probeFailed {exit(1)}
