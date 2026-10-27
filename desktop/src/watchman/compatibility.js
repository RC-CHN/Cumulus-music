// Scoped adapter for recovered collectors. No changes to global prototypes,
// renderer output, eval, event trust, cookies or anti-automation results.
(declaration) => {
  const actual = window.navigator;
  const declared = Object.freeze({
    userAgent: declaration.userAgent,
    appVersion: declaration.appVersion,
    platform: declaration.platform,
    language: declaration.language,
    languages: Object.freeze(declaration.languages.slice()),
    // Chrome omits these legacy IE/Firefox properties on Windows.
    oscpu: undefined, cpuClass: undefined, appMinorVersion: undefined,
    systemLanguage: undefined, userLanguage: undefined, browserLanguage: undefined
  });
  const navigator = new Proxy(actual, {
    get(target, key) {
      if (Object.prototype.hasOwnProperty.call(declared, key)) return declared[key];
      const value = Reflect.get(target, key, target);
      return typeof value === 'function' ? value.bind(target) : value;
    },
    set() { return false; },
    defineProperty() { return false; },
    deleteProperty() { return false; }
  });
  return Object.freeze({navigator});
}
