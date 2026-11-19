// Scoped adapter for recovered collectors.
//
// Two modes, chosen by whether the declaration carries a hardware picture:
//
//  * Declaration only (default). A read-only proxy feeds the collector its
//    Windows navigator fields. Global prototypes, eval, renderer output, event
//    trust, cookies and anti-automation results stay untouched.
//
//  * Full identity (declaration.hardware present). The page really becomes the
//    declared identity: Function.prototype.toString reports Chromium's native
//    eval form, Navigator/Screen/Date answer with the derived picture. This is
//    deliberate page identity substitution, not recovered client behaviour, and
//    callers must opt in explicitly through WindowsBrowserIdentity::with_hardware.
//
// Neither mode touches canvas or WebGL output. Measured across three platforms,
// those digests follow the GPU, not the platform, so faking them would create a
// hardware identity that never existed.
(declaration) => {
  const hardware = declaration.hardware;

  const navigatorKeys = {
    userAgent: declaration.userAgent,
    appVersion: declaration.appVersion,
    platform: declaration.platform,
    language: declaration.language,
    languages: Object.freeze(declaration.languages.slice()),
    oscpu: undefined, cpuClass: undefined, appMinorVersion: undefined,
    systemLanguage: undefined, userLanguage: undefined, browserLanguage: undefined
  };
  const navigator = new Proxy(window.navigator, {
    get(target, key) {
      if (Object.prototype.hasOwnProperty.call(navigatorKeys, key)) return navigatorKeys[key];
      const value = Reflect.get(target, key, target);
      return typeof value === 'function' ? value.bind(target) : value;
    },
    set() { return false; },
    defineProperty() { return false; },
    deleteProperty() { return false; }
  });

  if (hardware) {
    // Chromium reports "function eval() { [native code] }", exactly 33 characters.
    // WebKit's own form is 37, which is the whole of the recovered `ob` check.
    const chromiumEval = 'function eval() { [native code] }';
    const nativeToString = Function.prototype.toString;
    const patched = function toString() {
      try { if (this === eval) return chromiumEval; } catch (_) {}
      return nativeToString.call(this);
    };
    Object.defineProperty(Function.prototype, 'toString', {
      value: patched, writable: true, configurable: true, enumerable: false
    });

    const define = (target, key, value) => {
      try {
        Object.defineProperty(target, key, {
          get: () => value, configurable: true, enumerable: true
        });
      } catch (_) {}
    };

    for (const key of ['userAgent', 'appVersion', 'platform', 'language', 'languages']) {
      define(Navigator.prototype, key, navigatorKeys[key]);
    }
    define(Navigator.prototype, 'hardwareConcurrency', hardware.hardwareConcurrency);
    define(Navigator.prototype, 'maxTouchPoints', 0);

    for (const key of ['width', 'height', 'availWidth', 'availHeight']) {
      define(Screen.prototype, key, hardware[key]);
    }
    define(window, 'devicePixelRatio', hardware.devicePixelRatio);
    define(Screen.prototype, 'colorDepth', 24);
    define(Screen.prototype, 'pixelDepth', 24);

    const offset = hardware.timezoneOffset;
    try {
      Date.prototype.getTimezoneOffset = function getTimezoneOffset() { return offset; };
    } catch (_) {}
  }

  return Object.freeze({navigator});
}
