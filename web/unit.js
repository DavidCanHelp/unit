// unit.js — WASM glue + browser mesh for the unit Forth nanobot

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

  isRunning() { return this.exports.is_running(this.vmPtr) !== 0; }
  destroy() { this.exports.destroy(this.vmPtr); this.vmPtr = null; }
}

// =========================================================================
// Browser Mesh — multiple WASM VMs communicating via JS message bus
// =========================================================================

class BrowserUnit {
  constructor(vm, id) {
    this.vm = vm;
    this.id = id;
    this.fitness = 0;
    this.tasksCompleted = 0;
    this.busy = false;
    this.learned = [];     // words received from other units
    this.personality = ''; // specialist/balanced/solo
  }
}

class BrowserMesh {
  constructor() {
    this.units = [];
    this.wasmBytes = null;
    this.maxUnits = 5;
    this.onEvent = null;
    this.goalQueue = [];
  }

  async init(wasmPath) {
    const response = await fetch(wasmPath);
    this.wasmBytes = await response.arrayBuffer();
    // Create the first unit.
    const unit = await this._spawn();
    return unit;
  }

  async _spawn() {
    if (this.units.length >= this.maxUnits) return null;
    const { instance } = await WebAssembly.instantiate(this.wasmBytes.slice(0), { env: {} });
    const vm = new UnitVM(instance);
    const id = this._genId();
    const unit = new BrowserUnit(vm, id);
    // Inject browser mesh peer count into the Forth VM so JOY, LONELY, etc. work.
    vm.eval('VARIABLE BROWSER-PEERS 0 BROWSER-PEERS !');
    vm.eval(': PEER-COUNT BROWSER-PEERS @ ;');
    // Re-eval words that captured old PEER-COUNT at compile time (standard Forth
    // behavior — redefining a word doesn't patch existing references).
    vm.eval(': JOYFUL PEER-COUNT 0 > ;');
    vm.eval(': JOY JOYFUL IF ." I feel joy! " PEER-COUNT . ." peers in my mesh." CR ." Together we are more than alone." CR ELSE ." Joy requires connection. I have no peers yet." CR THEN ;');
    vm.eval(': HOW-ARE-YOU JOYFUL IF FITNESS DUP 50 > IF DROP ." joyful and thriving! fitness=" FITNESS . ." with " PEER-COUNT . ." peers" CR ELSE DUP 20 > IF DROP ." joyful. doing well. fitness=" FITNESS . ." with " PEER-COUNT . ." peers" CR ELSE DUP 10 > IF DROP ." getting started. fitness=" FITNESS . CR ELSE DUP 0 > IF DROP ." warming up. fitness=" FITNESS . CR ELSE DROP ." just spawned. finding my role. fitness=" FITNESS . CR THEN THEN THEN THEN ELSE FITNESS DUP 50 > IF DROP ." thriving solo. fitness=" FITNESS . CR ELSE DUP 20 > IF DROP ." doing okay solo. fitness=" FITNESS . CR ELSE DUP 10 > IF DROP ." getting started. fitness=" FITNESS . CR ELSE DUP 0 > IF DROP ." warming up. fitness=" FITNESS . CR ELSE DROP ." alone and new. fitness=" FITNESS . CR THEN THEN THEN THEN THEN ;');
    vm.eval(': LONELY PEER-COUNT 0 = IF ." I\'m alone. No peers in sight." CR ELSE ." I have " PEER-COUNT . ." friends!" CR THEN ;');
    vm.eval(': HEADCOUNT PEER-COUNT 1 + . ." units in the mesh" CR ;');
    vm.eval(': HELLO ." Hi! I\'m unit " ID TYPE ." , generation " GENERATION . ." with " PEER-COUNT . ." peers and fitness " FITNESS . CR ;');
    vm.eval(': INTROSPECT HOW-ARE-YOU OBS-COUNT @ DUP 0 > IF ." adapted " . ." times." CR ELSE DROP THEN ;');
    this.units.push(unit);
    this._updatePeerCounts();
    this._emit('spawn', { id, count: this.units.length });
    return unit;
  }

  async spawn(parentUnit) {
    if (this.units.length >= this.maxUnits) return null;
    const child = await this._spawn();
    if (child && parentUnit) this._inheritWords(parentUnit, child);
    else if (child && this.units.length > 1) this._inheritWords(this.units[0], child);
    return child;
  }

  _inheritWords(parent, child) {
    const genome = parent.vm.eval('EXPORT-GENOME').trim();
    if (!genome) return;
    for (const line of genome.split('\n')) {
      const def = line.trim();
      if (def.startsWith(':') && def.endsWith(';')) {
        child.vm.eval(def);
      }
    }
  }

  _genId() {
    return Math.random().toString(16).substring(2, 6);
  }

  _updatePeerCounts() {
    const peers = this.units.length - 1;
    for (const u of this.units) {
      u.vm.eval(`${peers} BROWSER-PEERS !`);
    }
  }

  _emit(type, data) {
    if (this.onEvent) this.onEvent(type, data);
  }

  // Pick the least-busy unit (excluding unit 0 which is the REPL).
  _pickWorker() {
    let best = null, bestScore = Infinity;
    for (let i = 0; i < this.units.length; i++) {
      const u = this.units[i];
      if (u.busy) continue;
      const score = u.tasksCompleted;
      if (score < bestScore) { bestScore = score; best = u; }
    }
    return best || this.units[0];
  }

  // Execute a goal on the best available unit. Returns {unitId, output, stack}.
  executeGoal(code) {
    const worker = this._pickWorker();
    worker.busy = true;
    this._emit('goal_start', { unitId: worker.id, code });

    const output = worker.vm.eval(code);
    // Get stack top by evaluating DEPTH.
    const depthOut = worker.vm.eval('.S');

    worker.busy = false;
    worker.tasksCompleted++;
    worker.fitness += 15;
    this._emit('goal_done', { unitId: worker.id, code, output, stack: depthOut });
    return { unitId: worker.id, output, stack: depthOut };
  }

  // Share a word definition with all units.
  shareWord(definition) {
    for (const u of this.units) {
      u.vm.eval(definition);
    }
    this._emit('word_shared', { definition, count: this.units.length });
  }

  // Teach: one unit shares words it invented with all others.
  teachFrom(sourceUnit) {
    const wordsToShare = ['MY-ROUTINE', 'GREET', 'MY-STRATEGY'];
    let taught = [];
    for (const wordName of wordsToShare) {
      const seeDef = sourceUnit.vm.eval('SEE ' + wordName);
      if (seeDef.includes(':') && seeDef.includes(';')) {
        for (const target of this.units) {
          if (target === sourceUnit) continue;
          target.vm.eval(seeDef.trim());
          if (!target.learned.includes(wordName)) target.learned.push(wordName);
        }
        taught.push(wordName);
      }
    }
    // Detect personality from strategy output.
    const stratOut = sourceUnit.vm.eval('INVENT-STRATEGY');
    if (stratOut.includes('specialist')) sourceUnit.personality = 'specialist';
    else if (stratOut.includes('balanced')) sourceUnit.personality = 'balanced';
    else sourceUnit.personality = 'solo';

    if (taught.length > 0) {
      this._emit('teach', { from: sourceUnit.id, words: taught });
    }
    return taught;
  }

  // Get mesh status.
  status() {
    return {
      count: this.units.length,
      units: this.units.map(u => ({
        id: u.id, fitness: u.fitness,
        tasks: u.tasksCompleted, busy: u.busy,
        personality: u.personality, learned: u.learned.length
      }))
    };
  }
}
