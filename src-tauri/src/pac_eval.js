// Evaluates a proxy auto-config (PAC) file with macOS's built-in JavaScript
// engine (JXA). Run as:
//   osascript -l JavaScript -e "<this file>" <pac-path> <url> <host>
// Prints the FindProxyForURL result, e.g. "PROXY proxy.corp:911; DIRECT".
ObjC.import('Foundation');

function __readFile(path) {
  var s = $.NSString.stringWithContentsOfFileEncodingError(path, $.NSUTF8StringEncoding, null);
  return s.isNil() ? '' : ObjC.unwrap(s);
}
function __addresses(host) {
  var h = $.NSHost.hostWithName(String(host));
  if (!h || h.isNil()) return [];
  return ObjC.deepUnwrap(h.addresses) || [];
}
function __myAddresses() {
  return ObjC.deepUnwrap($.NSHost.currentHost.addresses) || [];
}

// ---- Standard PAC helpers ------------------------------------------------
var __V4 = /^\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}$/;
function __ip(a) {
  var p = String(a).split('.');
  if (p.length !== 4) return null;
  return (((+p[0]) << 24) >>> 0) + ((+p[1]) << 16) + ((+p[2]) << 8) + (+p[3]);
}
function dnsResolve(host) {
  if (__V4.test(host)) return host;
  var a = __addresses(host).filter(function (x) { return __V4.test(x); });
  return a.length ? a[0] : null;
}
function dnsResolveEx(host) { return __addresses(host).join(';'); }
function myIpAddress() {
  var a = __myAddresses().filter(function (x) { return __V4.test(x) && x.indexOf('127.') !== 0; });
  return a.length ? a[0] : '127.0.0.1';
}
function myIpAddressEx() { return __myAddresses().join(';'); }
function isResolvable(host) { return dnsResolve(host) !== null; }
function isResolvableEx(host) { return dnsResolveEx(host) !== ''; }
function isPlainHostName(host) { return String(host).indexOf('.') < 0; }
function dnsDomainIs(host, domain) {
  host = String(host).toLowerCase(); domain = String(domain).toLowerCase();
  return host.length >= domain.length && host.substring(host.length - domain.length) === domain;
}
function localHostOrDomainIs(host, hostdom) {
  return host === hostdom || String(hostdom).lastIndexOf(host + '.', 0) === 0;
}
function dnsDomainLevels(host) { return String(host).split('.').length - 1; }
function convert_addr(ip) { return __ip(ip); }
function isInNet(host, pattern, mask) {
  var ip = dnsResolve(host);
  if (!ip) return false;
  var m = __ip(mask);
  return ((__ip(ip) & m) >>> 0) === ((__ip(pattern) & m) >>> 0);
}
function isInNetEx(host, prefix) {
  var parts = String(prefix).split('/');
  var bits = +parts[1];
  if (!__V4.test(parts[0]) || !(bits >= 0 && bits <= 32)) return false;
  var m = bits === 0 ? 0 : ((0xffffffff << (32 - bits)) >>> 0);
  var ip = dnsResolve(host);
  if (!ip) return false;
  return ((__ip(ip) & m) >>> 0) === ((__ip(parts[0]) & m) >>> 0);
}
function shExpMatch(str, pattern) {
  var re = '^' + String(pattern).replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*').replace(/\?/g, '.') + '$';
  return new RegExp(re).test(String(str));
}
// Time-based rules are rare in corporate PACs; treat them as always matching.
function weekdayRange() { return true; }
function dateRange() { return true; }
function timeRange() { return true; }
function alert() {}

function run(argv) {
  var pac = __readFile(argv[0]);
  (0, eval)(pac);
  return String(FindProxyForURL(argv[1], argv[2]));
}
