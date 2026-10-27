// Generated measurement only: 602a5ad7 Jb; source SHA256 343a187cb023c495a2dffee37f8265d203d97886c43fa6180951fb2de4d283b5.
(context) => {
  try {
    const navigator=context?.navigator ?? window.navigator;
    const read = name => { try { return window[name]; } catch (_) {} };
    const y=window,x=document,w=navigator; let ob,vb,Da,hc;
    const qd=read('WebGLRenderingContext'),fd=read('ActiveXObject');
    let hashed=false;
    function za(input) {
      if (typeof input !== 'string') throw new TypeError('fingerprint input must be text');
      hashed=true; return input;
    }
function ed() {
          var a = [];
          if (Object["getOwnPropertyDescriptor"] && Object["getOwnPropertyDescriptor"](y, "ActiveXObject") || "ActiveXObject" in y) a = ["AcroPDF.PDF", "Adodb.Stream", "AgControl.AgControl", "DevalVRXCtrl.DevalVRXCtrl.1", "MacromediaFlashPaper.MacromediaFlashPaper", "Msxml2.DOMDocument", "Msxml2.XMLHTTP", "PDF.PdfCtrl", "QuickTime.QuickTime", "QuickTimeCheckObject.QuickTimeCheck.1", "RealPlayer", "RealPlayer.RealPlayer(tm) ActiveX Control (32-bit)", "RealVideo.RealVideo(tm) ActiveX Control (32-bit)", "Scripting.Dictionary", "SWCtl.SWCtl", "Shell.UIHelper", "ShockwaveFlash.ShockwaveFlash", "Skype.Detection", "TDCCtl.TDCCtl", "WMPlayer.OCX", "rmocx.RealPlayer G2 Control", "rmocx.RealPlayer G2 Control.1"].map(function (a) {
            try {
              return new fd(a), a;
            } catch (b) {
              return null;
            }
          });
          w["plugins"] && (a = a.concat(kc()));
          return a;
        }
function kc() {
          var c = [],
            c = [].slice.call(w["plugins"], 0);
          return c.map(function (c) {
            var d = [].slice.call(c, 0).map(function (a) {
              return [a.type, a["suffixes"]].join("~");
            }).join(",");
            return [c.name, c["description"], d].join("::");
          });
        }
function gd() {
          return w["appName"] === "Microsoft Internet Explorer" || w["appName"] === "Netscape" && /Trident/.test(w["userAgent"]) ? !0 : !1;
        }
    const value=(function () {
                var a = gd ? ed() : kc();
                return za(a.join("~"));
              })();
    if (hashed) return {kind:'fingerprint',preimage:value};
    if (value === '') return {kind:'unsupported'};
    throw new TypeError('unexpected measurement result');
  } catch (_) { return {kind:'failed'}; }
}
