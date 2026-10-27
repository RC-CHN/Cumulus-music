// Generated measurement only: af2952a4 dc; source SHA256 af456401a96e7cab2fc0ad1334e1014ed58d5c694908d2d73ec1ef426463f787.
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
function kc() {
          var a = w["createElement"]("canvas");
          return !(!a["getContext"] || !a["getContext"]("2d"));
        }
function bd() {
          var a = w["createElement"]("canvas"),
            c = null;
          try {
            c = a["getContext"]("webgl") || a["getContext"]("experimental-webgl");
          } catch (d) {}
          c || (c = null);
          return c;
        }
function cd() {
          function e(e) {
            c["clearColor"](0, 0, 0, 1);
            c.enable(c["DEPTH_TEST"]);
            c["depthFunc"](c["LEQUAL"]);
            c.clear(c["COLOR_BUFFER_BIT"] | c["DEPTH_BUFFER_BIT"]);
            return "[" + e[0] + ", " + e[1] + "]";
          }
          if (Ba) return Ba;
          var c;
          c = bd();
          if (!c) return Ba = [];
          var d = [];
          try {
            var l = "attribute vec2 attrVertexvarying vec2 varyinTexCoordinateuniform vec2 uniformOffsetvoid main(){varyinTexCoordinate=attrVertex+uniformOffsetgl_Position=vec4(attrVertex,0,1)}",
              f = "precision mediump floatvarying vec2 varyinTexCoordinatevoid main() {gl_FragColor=vec4(varyinTexCoordinate,0,1)}",
              h = c["createBuffer"]();
            c["bindBuffer"](c["ARRAY_BUFFER"], h);
            var n = new Float32Array([-0.2, -0.9, 0, 0.4, -0.26, 0, 0, 0.732134444, 0]);
            c.bufferData(c["ARRAY_BUFFER"], n, c["STATIC_DRAW"]);
            h.wc = 3;
            h.Ac = 3;
            var k = c["createProgram"](),
              m = c["createShader"](c["VERTEX_SHADER"]);
            c["shaderSource"](m, l);
            c["compileShader"](m);
            var p = c["createShader"](c["FRAGMENT_SHADER"]);
            c["shaderSource"](p, f);
            c["compileShader"](p);
            c["attachShader"](k, m);
            c["attachShader"](k, p);
            c["linkProgram"](k);
            c["useProgram"](k);
            k.Hc = c["getAttribLocation"](k, "attrVertex");
            k.Bc = c["getUniformLocation"](k, "uniformOffset");
            c["enableVertexAttribArray"](k.Qc);
            c["vertexAttribPointer"](k.Hc, h.wc, c.FLOAT, !1, 0, 0);
            c["uniform2f"](k.Bc, 1, 1);
            c["drawArrays"](c["TRIANGLE_STRIP"], 0, h.Ac);
          } catch (t) {}
          null != c["canvas"] && d.push(c["canvas"]["toDataURL"]());
          d.push("extensions:" + c["getSupportedExtensions"]().join(""));
          d.push("webgl aliased line width range:" + e(c["getParameter"](c["ALIASED_LINE_WIDTH_RANGE"])));
          d.push("webgl aliased point size range:" + e(c["getParameter"](c["ALIASED_POINT_SIZE_RANGE"])));
          d.push("webgl alpha bits:" + c["getParameter"](c["ALPHA_BITS"]));
          d.push("webgl antialiasing:" + (c["getContextAttributes"]().antialias ? "yes" : "no"));
          d.push("webgl blue bits:" + c["getParameter"](c["BLUE_BITS"]));
          d.push("webgl depth bits:" + c["getParameter"](c["DEPTH_BITS"]));
          d.push("webgl green bits:" + c["getParameter"](c["GREEN_BITS"]));
          d.push("webgl max anisotropy:" + function (e) {
            var c,
              d = e["getExtension"]("EXT_texture_filter_anisotropic") || e["getExtension"]("WEBKIT_EXT_texture_filter_anisotropic") || e["getExtension"]("MOZ_EXT_texture_filter_anisotropic");
            return d ? (c = e["getParameter"](d["MAX_TEXTURE_MAX_ANISOTROPY_EXT"]), 0 === c && (c = 2), c) : null;
          }(c));
          d.push("webgl max combined texture image units:" + c["getParameter"](c["MAX_COMBINED_TEXTURE_IMAGE_UNITS"]));
          d.push("webgl max cube map texture size:" + c["getParameter"](c["MAX_CUBE_MAP_TEXTURE_SIZE"]));
          d.push("webgl max fragment uniform vectors:" + c["getParameter"](c["MAX_FRAGMENT_UNIFORM_VECTORS"]));
          d.push("webgl max render buffer size:" + c["getParameter"](c["MAX_RENDERBUFFER_SIZE"]));
          d.push("webgl max texture image units:" + c["getParameter"](c["MAX_TEXTURE_IMAGE_UNITS"]));
          d.push("webgl max texture size:" + c["getParameter"](c["MAX_TEXTURE_SIZE"]));
          d.push("webgl max varying vectors:" + c["getParameter"](c["MAX_VARYING_VECTORS"]));
          d.push("webgl max vertex attribs:" + c["getParameter"](c["MAX_VERTEX_ATTRIBS"]));
          d.push("webgl max vertex texture image units:" + c["getParameter"](c["MAX_VERTEX_TEXTURE_IMAGE_UNITS"]));
          d.push("webgl max vertex uniform vectors:" + c["getParameter"](c["MAX_VERTEX_UNIFORM_VECTORS"]));
          d.push("webgl max viewport dims:" + e(c["getParameter"](c["MAX_VIEWPORT_DIMS"])));
          d.push("webgl red bits:" + c["getParameter"](c["RED_BITS"]));
          d.push("webgl renderer:" + c["getParameter"](c["RENDERER"]));
          d.push("webgl shading language version:" + c["getParameter"](c["SHADING_LANGUAGE_VERSION"]));
          d.push("webgl stencil bits:" + c["getParameter"](c["STENCIL_BITS"]));
          d.push("webgl vendor:" + c["getParameter"](c["VENDOR"]));
          d.push("webgl version:" + c["getParameter"](c["VERSION"]));
          try {
            var u = c["getExtension"]("WEBGL_debug_renderer_info");
            u && (d.push("webgl unmasked vendor:" + c["getParameter"](u.UNMASKED_VENDOR_WEBGL)), d.push("webgl unmasked renderer:" + c["getParameter"](u.UNMASKED_RENDERER_WEBGL)));
          } catch (w) {}
          if (!c["getShaderPrecisionFormat"]) return Ba = d;
          d.push("webgl vertex shader high float precision:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["HIGH_FLOAT"])["precision"]);
          d.push("webgl vertex shader high float precision rangeMin:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["HIGH_FLOAT"])["rangeMin"]);
          d.push("webgl vertex shader high float precision rangeMax:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["HIGH_FLOAT"])["rangeMax"]);
          d.push("webgl vertex shader medium float precision:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["MEDIUM_FLOAT"])["precision"]);
          d.push("webgl vertex shader medium float precision rangeMin:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["MEDIUM_FLOAT"])["rangeMin"]);
          d.push("webgl vertex shader medium float precision rangeMax:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["MEDIUM_FLOAT"])["rangeMax"]);
          d.push("webgl vertex shader low float precision:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["LOW_FLOAT"])["precision"]);
          d.push("webgl vertex shader low float precision rangeMin:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["LOW_FLOAT"])["rangeMin"]);
          d.push("webgl vertex shader low float precision rangeMax:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["LOW_FLOAT"])["rangeMax"]);
          d.push("webgl fragment shader high float precision:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["HIGH_FLOAT"])["precision"]);
          d.push("webgl fragment shader high float precision rangeMin:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["HIGH_FLOAT"])["rangeMin"]);
          d.push("webgl fragment shader high float precision rangeMax:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["HIGH_FLOAT"])["rangeMax"]);
          d.push("webgl fragment shader medium float precision:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["MEDIUM_FLOAT"])["precision"]);
          d.push("webgl fragment shader medium float precision rangeMin:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["MEDIUM_FLOAT"])["rangeMin"]);
          d.push("webgl fragment shader medium float precision rangeMax:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["MEDIUM_FLOAT"])["rangeMax"]);
          d.push("webgl fragment shader low float precision:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["LOW_FLOAT"])["precision"]);
          d.push("webgl fragment shader low float precision rangeMin:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["LOW_FLOAT"])["rangeMin"]);
          d.push("webgl fragment shader low float precision rangeMax:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["LOW_FLOAT"])["rangeMax"]);
          d.push("webgl vertex shader high int precision:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["HIGH_INT"])["precision"]);
          d.push("webgl vertex shader high int precision rangeMin:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["HIGH_INT"])["rangeMin"]);
          d.push("webgl vertex shader high int precision rangeMax:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["HIGH_INT"])["rangeMax"]);
          d.push("webgl vertex shader medium int precision:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["MEDIUM_INT"])["precision"]);
          d.push("webgl vertex shader medium int precision rangeMin:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["MEDIUM_INT"])["rangeMin"]);
          d.push("webgl vertex shader medium int precision rangeMax:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["MEDIUM_INT"])["rangeMax"]);
          d.push("webgl vertex shader low int precision:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["LOW_INT"])["precision"]);
          d.push("webgl vertex shader low int precision rangeMin:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["LOW_INT"])["rangeMin"]);
          d.push("webgl vertex shader low int precision rangeMax:" + c["getShaderPrecisionFormat"](c["VERTEX_SHADER"], c["LOW_INT"])["rangeMax"]);
          d.push("webgl fragment shader high int precision:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["HIGH_INT"])["precision"]);
          d.push("webgl fragment shader high int precision rangeMin:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["HIGH_INT"])["rangeMin"]);
          d.push("webgl fragment shader high int precision rangeMax:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["HIGH_INT"])["rangeMax"]);
          d.push("webgl fragment shader medium int precision:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["MEDIUM_INT"])["precision"]);
          d.push("webgl fragment shader medium int precision rangeMin:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["MEDIUM_INT"])["rangeMin"]);
          d.push("webgl fragment shader medium int precision rangeMax:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["MEDIUM_INT"])["rangeMax"]);
          d.push("webgl fragment shader low int precision:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["LOW_INT"])["precision"]);
          d.push("webgl fragment shader low int precision rangeMin:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["LOW_INT"])["rangeMin"]);
          d.push("webgl fragment shader low int precision rangeMax:" + c["getShaderPrecisionFormat"](c["FRAGMENT_SHADER"], c["LOW_INT"])["rangeMax"]);
          return Ba = d;
        }
    const value=(function () {
                var a;
                if (kc()) {
                  a = w["createElement"]("canvas");
                  var c;
                  try {
                    c = a["getContext"] && (a["getContext"]("webgl") || a["getContext"]("experimental-webgl"));
                  } catch (d) {
                    c = !1;
                  }
                  a = !!qd && !!c;
                } else a = !1;
                return a ? xa(cd().join("~")) : "";
              })();
    if (hashed) return {kind:'fingerprint',preimage:value};
    if (value === '') return {kind:'unsupported'};
    throw new TypeError('unexpected measurement result');
  } catch (_) { return {kind:'failed'}; }
}
