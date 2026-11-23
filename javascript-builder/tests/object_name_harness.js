// Runs a rendered client script in Node with a minimal stand in for a
// browser, and prints what the script created under a given object name.
//
// Usage: node object_name_harness.js <script file> <object name> [value]
//
// [value] is an optional dotted path under the object, such as
// "device.ismobile", whose value is printed as JSON.
//
// The output is one line of JSON. Only the global object, its public
// functions and one payload value are read, which is enough to show that a
// script rendered with a different object name works. The script has no
// callback URL in these tests, so it never makes a request.
'use strict';
const fs = require('fs');
const vm = require('vm');

const script = fs.readFileSync(process.argv[2], 'utf8');
const name = process.argv[3];
const valuePath = process.argv[4];

// Session storage with the members the script reads.
function makeStorage() {
    const data = {};
    const methods = {
        getItem: (k) => Object.prototype.hasOwnProperty.call(data, String(k))
            ? data[String(k)] : null,
        setItem: (k, v) => { data[String(k)] = String(v); },
        removeItem: (k) => { delete data[String(k)]; },
        key: (i) => Object.keys(data)[i] === undefined
            ? null : Object.keys(data)[i],
        clear: () => Object.keys(data).forEach((k) => delete data[k])
    };
    return new Proxy(data, {
        get(t, p) {
            if (p === 'length') { return Object.keys(data).length; }
            if (Object.prototype.hasOwnProperty.call(methods, p)) {
                return methods[p];
            }
            return typeof p === 'string' ? data[p] : undefined;
        },
        set(t, p, v) { data[String(p)] = String(v); return true; }
    });
}

const log = [];
const sandbox = {
    console: {
        log: (...a) => log.push(a.join(' ')),
        warn: (...a) => log.push(a.join(' ')),
        error: (...a) => log.push(a.join(' '))
    },
    document: { cookie: '' },
    sessionStorage: makeStorage(),
    localStorage: makeStorage(),
    navigator: { userAgent: 'node' },
    setTimeout: setTimeout,
    clearTimeout: clearTimeout,
    addEventListener: () => {},
    removeEventListener: () => {}
};
const context = vm.createContext(sandbox);
// The global object is the window, as it is in a browser.
vm.runInContext('var window = this;', context);

let error = null;
try {
    vm.runInContext(script, context, { filename: 'rendered.js' });
} catch (err) {
    error = String(err && err.message);
}

const read = (expression) => {
    try {
        return vm.runInContext(expression, context);
    } catch (err) {
        return 'threw: ' + err.message;
    }
};
const quoted = JSON.stringify(name);
process.stdout.write(JSON.stringify({
    error: error,
    exists: read('typeof window[' + quoted + ']') === 'object',
    complete: read('typeof window[' + quoted + '].complete'),
    onChange: read('typeof window[' + quoted + '].onChange'),
    refresh: read('typeof window[' + quoted + '].refresh'),
    value: valuePath === undefined ? undefined : read(
        'JSON.stringify(window[' + quoted + ']' + valuePath.split('.')
            .map((part) => '[' + JSON.stringify(part) + ']').join('') + ')'),
    globals: Object.keys(sandbox).filter((k) =>
        typeof sandbox[k] === 'object' && sandbox[k] !== null &&
        typeof sandbox[k].complete === 'function')
}) + '\n');
