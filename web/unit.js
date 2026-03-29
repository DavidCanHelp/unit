// unit.js — WASM glue for the unit Forth nanobot

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
    const { instance } = await WebAssembly.instantiate(bytes, { env: {} });
    return new UnitVM(instance);
  }

  eval(line) {
    const inputBytes = this.encoder.encode(line);
    const inputPtr = this.exports.alloc(inputBytes.length);
    const mem = new Uint8Array(this.exports.memory.buffer);
    mem.set(inputBytes, inputPtr);
    const outputPtr = this.exports.eval(this.vmPtr, inputPtr, inputBytes.length);
    const outputMem = new Uint8Array(this.exports.memory.buffer);
    let end = outputPtr;
    while (outputMem[end] !== 0) end++;
    const output = this.decoder.decode(outputMem.slice(outputPtr, end));
    this.exports.dealloc(inputPtr, inputBytes.length);
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
