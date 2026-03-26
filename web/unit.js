// unit.js — WASM glue for the unit Forth nanobot
//
// Bridges the WASM unit VM to browser I/O. Handles memory management
// for string passing between JavaScript and the WASM linear memory.

class UnitVM {
  constructor(instance) {
    this.instance = instance;
    this.exports = instance.exports;
    this.vmPtr = this.exports.boot();
    this.encoder = new TextEncoder();
    this.decoder = new TextDecoder();
  }

  static async create(wasmPath) {
    const response = await fetch(wasmPath);
    const bytes = await response.arrayBuffer();
    const { instance } = await WebAssembly.instantiate(bytes, {
      env: {
        // Stubs for any extern functions the WASM binary expects.
        // Add browser API bridges here as needed.
      }
    });
    return new UnitVM(instance);
  }

  // Evaluate a line of Forth. Returns the captured output string.
  eval(line) {
    const inputBytes = this.encoder.encode(line);
    const inputPtr = this.exports.alloc(inputBytes.length);

    // Write input string into WASM memory.
    const mem = new Uint8Array(this.exports.memory.buffer);
    mem.set(inputBytes, inputPtr);

    // Call eval — returns pointer to NUL-terminated output string.
    const outputPtr = this.exports.eval(this.vmPtr, inputPtr, inputBytes.length);

    // Read output string from WASM memory.
    const outputMem = new Uint8Array(this.exports.memory.buffer);
    let end = outputPtr;
    while (outputMem[end] !== 0) end++;
    const output = this.decoder.decode(outputMem.slice(outputPtr, end));

    // Free allocated memory.
    this.exports.dealloc(inputPtr, inputBytes.length);
    // Note: output string is leaked for simplicity. In production,
    // we'd track and free it too.

    return output;
  }

  isRunning() {
    return this.exports.is_running(this.vmPtr) !== 0;
  }

  destroy() {
    this.exports.destroy(this.vmPtr);
    this.vmPtr = null;
  }
}
