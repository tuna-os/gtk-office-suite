#!/usr/bin/env python3
"""Mandelbrot risk gate: probe WebKitGTK for the WebRTC surface Element Call needs.

Run inside the GNOME Platform runtime:
  xvfb-run -a flatpak run --socket=x11 --share=ipc --device=dri \
    --filesystem=/path/to/prototypes --command=python3 \
    org.gnome.Platform//50 prototypes/webrtc-probe.py
"""
import sys

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("WebKit", "6.0")
from gi.repository import GLib, Gtk, WebKit

HTML = """<!doctype html><html><body><script>
async function probe() {
  const out = { userAgent: navigator.userAgent };
  out.rtcPeerConnection = typeof RTCPeerConnection !== 'undefined';
  out.rtcRtpScriptTransform = typeof RTCRtpScriptTransform !== 'undefined';
  out.encodedStreams = !!(window.RTCRtpSender && RTCRtpSender.prototype.createEncodedStreams);
  out.getDisplayMedia = !!(navigator.mediaDevices && navigator.mediaDevices.getDisplayMedia);
  out.audioWorklet = typeof AudioWorkletNode !== 'undefined';
  out.wasm = typeof WebAssembly !== 'undefined';
  out.webCodecsVideoEncoder = typeof VideoEncoder !== 'undefined';
  out.insertableStreamsWorker = typeof Worker !== 'undefined';
  try {
    const caps = RTCRtpSender.getCapabilities ? RTCRtpSender.getCapabilities('video') : null;
    out.videoCodecs = caps ? [...new Set(caps.codecs.map(c => c.mimeType))] : null;
  } catch (e) { out.videoCodecs = 'ERROR: ' + e; }
  try {
    const s = await navigator.mediaDevices.getUserMedia({ audio: true, video: true });
    out.getUserMedia = 'OK: ' + s.getTracks().map(t => t.kind).sort().join('+');
    s.getTracks().forEach(t => t.stop());
  } catch (e) { out.getUserMedia = 'FAIL: ' + e.name + ' ' + e.message; }
  try {
    const pc = new RTCPeerConnection();
    const offer = await pc.createOffer({ offerToReceiveAudio: true, offerToReceiveVideo: true });
    out.sdpOfferHasMedia = /m=video/.test(offer.sdp) && /m=audio/.test(offer.sdp);
    pc.close();
  } catch (e) { out.sdpOfferHasMedia = 'FAIL: ' + e; }
  window.webkit.messageHandlers.probe.postMessage(JSON.stringify(out, null, 2));
}
probe();
</script></body></html>"""


def main():
    app = Gtk.Application(application_id="org.mandelbrot.WebRtcProbe")
    exit_code = [1]

    def on_activate(app):
        win = Gtk.ApplicationWindow(application=app, default_width=800, default_height=600)
        ucm = WebKit.UserContentManager()

        def on_message(_ucm, js_value):
            print(js_value.to_string())
            exit_code[0] = 0
            app.quit()

        ucm.register_script_message_handler("probe", None)
        ucm.connect("script-message-received::probe", on_message)

        view = WebKit.WebView(user_content_manager=ucm)
        settings = view.get_settings()
        settings.set_enable_webrtc(True)
        settings.set_enable_media_stream(True)
        settings.set_enable_mock_capture_devices(True)
        settings.set_enable_developer_extras(True)

        features = settings.get_all_features()
        for i in range(features.get_length()):
            feat = features.get(i)
            ident = feat.get_identifier()
            if any(k in ident.lower() for k in ("rtc", "peer", "webcodecs", "capture")):
                print(f"feature {ident}: default={feat.get_default_value()} -> enabling",
                      file=sys.stderr)
                settings.set_feature_enabled(feat, True)

        def on_permission(_view, request):
            request.allow()
            return True

        view.connect("permission-request", on_permission)
        win.set_child(view)
        win.present()
        view.load_html(HTML, "https://probe.invalid/")

        def timeout():
            print("TIMEOUT: probe did not report within 45s", file=sys.stderr)
            app.quit()
            return GLib.SOURCE_REMOVE

        GLib.timeout_add_seconds(45, timeout)

    app.connect("activate", on_activate)
    app.run(None)
    sys.exit(exit_code[0])


main()
