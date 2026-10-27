// Generated measurement only: af2952a4 Jb; source SHA256 af456401a96e7cab2fc0ad1334e1014ed58d5c694908d2d73ec1ef426463f787.
(context) => {
  try {
    const navigator=context?.navigator ?? window.navigator;
    const read = name => { try { return window[name]; } catch (_) {} };
    const x=window,w=document,u=navigator; let mb,tb,Ba,gc;
    const qd=read('WebGLRenderingContext'),fd=read('ActiveXObject');
    let hashed=false;
    function xa(input) {
      if (typeof input !== 'string') throw new TypeError('fingerprint input must be text');
      hashed=true; return input;
    }
function ed() {
          var a = [];
          if (Object["getOwnPropertyDescriptor"] && Object["getOwnPropertyDescriptor"](x, "ActiveXObject") || "ActiveXObject" in x) a = ["AcroPDF.PDF", "Adodb.Stream", "AgControl.AgControl", "DevalVRXCtrl.DevalVRXCtrl.1", "MacromediaFlashPaper.MacromediaFlashPaper", "Msxml2.DOMDocument", "Msxml2.XMLHTTP", "PDF.PdfCtrl", "QuickTime.QuickTime", "QuickTimeCheckObject.QuickTimeCheck.1", "RealPlayer", "RealPlayer.RealPlayer(tm) ActiveX Control (32-bit)", "RealVideo.RealVideo(tm) ActiveX Control (32-bit)", "Scripting.Dictionary", "SWCtl.SWCtl", "Shell.UIHelper", "ShockwaveFlash.ShockwaveFlash", "Skype.Detection", "TDCCtl.TDCCtl", "WMPlayer.OCX", "rmocx.RealPlayer G2 Control", "rmocx.RealPlayer G2 Control.1"].map(function (a) {
            try {
              return new fd(a), a;
            } catch (b) {
              return null;
            }
          });
          u["plugins"] && (a = a.concat(jc()));
          return a;
        }
function jc() {
          var e = [],
            e = [].slice.call(u["plugins"], 0);
          return e.map(function (e) {
            var d = [].slice.call(e, 0).map(function (a) {
              return [a.type, a["suffixes"]].join("~");
            }).join(",");
            return [e.name, e["description"], d].join("::");
          });
        }
function gd() {
          return u["appName"] === "Microsoft Internet Explorer" || u["appName"] === "Netscape" && /Trident/.test(u["userAgent"]) ? !0 : !1;
        }
    const value=(function () {
                var a = gd ? ed() : jc();
                return xa(a.join("~"));
              })();
    if (hashed) return {kind:'fingerprint',preimage:value};
    if (value === '') return {kind:'unsupported'};
    throw new TypeError('unexpected measurement result');
  } catch (_) { return {kind:'failed'}; }
}
