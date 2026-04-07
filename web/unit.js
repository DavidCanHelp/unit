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
    this.personality = 'newborn'; // overridden to 'self' for unit[0], then specialist/balanced/solo by teachFrom
    this.userWords = [];   // user-defined word definitions (Forth source)
    // Energy state
    this.energy = 1000;
    this.energyMax = 5000;
    this.energyEarned = 0;
    this.energySpent = 0;
  }
}

class BrowserMesh {
  constructor() {
    this.units = [];
    this.wasmBytes = null;
    this.maxUnits = 10;
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
    // Set unique personality seed per unit.
    vm.eval(`${this.units.length * 37 + 7} PERSONALITY-SEED !`);
    // Define MATE and related words as no-ops in WASM to prevent console errors.
    vm.eval(': MATE ." use MATE from the REPL" CR ;');
    vm.eval(': MATE-STATUS ." no mating in browser" CR ;');
    vm.eval(': ACCEPT-MATE ;');
    vm.eval(': DENY-MATE ;');
    vm.eval(': OFFSPRING ." no offspring from mating" CR ;');
    // Redefine DREAM dependencies that may produce empty output in WASM.
    // SHARE-ALL is a mesh primitive that silently no-ops when mesh is None;
    // redefine to give visible feedback. Also redefine DREAM itself to ensure
    // all nested ." output is captured in the WASM output buffer.
    vm.eval(': SHARE-ALL ." (no mesh peers to share with)" CR ;');
    vm.eval(': DREAM ." dreaming..." CR REFLECT INVENT-STRATEGY COMPOSE-ROUTINE SMART-MUTATE IF ." evolved." CR ELSE ." held steady." CR THEN MUTATION-REPORT PEER-COUNT 0 > IF TEACH THEN ." waking. I am changed." CR ;');
    // Sync native mesh primitives with browser mesh state.
    vm.eval(`: ID-STR S" ${id}" ;`);
    vm.eval(': ID ID-STR ;');
    vm.eval('VARIABLE BROWSER-FITNESS 0 BROWSER-FITNESS !');
    vm.eval(': FITNESS BROWSER-FITNESS @ ;');
    vm.eval(': PEERS PEER-COUNT ;');
    // Re-eval prelude words that use ID and FITNESS so they pick up the new definitions.
    vm.eval(': FAMILY ." id: " ID TYPE ."  gen: " GENERATION . ."  children: " CHILD-COUNT . CR ;');
    vm.eval(': HELLO ." Hi! I\'m unit " ID TYPE ." , generation " GENERATION . ." with " PEER-COUNT . ." peers and fitness " FITNESS . CR ;');
    vm.eval(': MESH-HELLO ." Mesh node " ID TYPE ."  gen=" GENERATION . ." peers=" PEER-COUNT . ." fitness=" FITNESS . CR ;');
    vm.eval(': PROUD ." fitness: " FITNESS . ." | generation: " GENERATION . ." | children: " CHILD-COUNT . CR ;');
    vm.eval(': ROLL-CALL ." === roll call ===" CR ." self: " ID TYPE ."  fitness=" FITNESS . CR LEADERBOARD ;');
    // Platform-limited words: give informative messages instead of silent failure.
    vm.eval(': SLEEP DROP ." sleep not available in browser" CR ;');
    vm.eval(': SPAWN ." spawn handled by browser mesh -- use the spawn button" CR ;');
    vm.eval(': CONNECT" DROP ." mesh connections handled by browser" CR ;');
    vm.eval(': DISCONNECT" DROP ." mesh connections handled by browser" CR ;');
    vm.eval(': DISCOVER ." discovery handled by browser mesh automatically" CR ;');
    vm.eval(': AUTO-DISCOVER ." auto-discovery is always on in the browser" CR ;');
    vm.eval(': SEXP-SEND" DROP ." use the browser mesh for messaging" CR ;');
    vm.eval(': SEXP-RECV ." use the browser mesh for messaging" CR ;');
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
    for (const def of parent.userWords) {
      child.vm.eval(def);
      if (!child.userWords.includes(def)) child.userWords.push(def);
    }
  }

  _genId() {
    return Math.random().toString(16).substring(2, 6);
  }

  _updatePeerCounts() {
    const peers = this.units.length - 1;
    for (const u of this.units) {
      u.vm.eval(`${peers} BROWSER-PEERS !`);
      u.vm.eval(`${u.fitness} BROWSER-FITNESS !`);
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

  // Extract user-defined genome (word definitions) from a unit.
  _extractGenome(unit) {
    const words = [];
    const seen = new Set();
    // Gather from userWords and learned arrays.
    for (const def of unit.userWords) {
      const m = def.match(/^:\s+(\S+)/);
      if (m && !seen.has(m[1])) { seen.add(m[1]); words.push({ name: m[1], definition: def }); }
    }
    for (const name of unit.learned) {
      if (seen.has(name)) continue;
      const seeDef = unit.vm.eval('SEE ' + name).trim();
      if (seeDef.startsWith(':') && seeDef.includes(';')) {
        seen.add(name); words.push({ name, definition: seeDef });
      }
    }
    // SOL-* words from WORDS output.
    const allWords = (unit.vm.eval('WORDS') || '').split(/\s+/).filter(w => w.startsWith('SOL-'));
    for (const name of allWords) {
      if (seen.has(name)) continue;
      const seeDef = unit.vm.eval('SEE ' + name).trim();
      if (seeDef.startsWith(':') && seeDef.includes(';')) {
        seen.add(name); words.push({ name, definition: seeDef });
      }
    }
    return words;
  }

  // Tournament selection: pick 3 random eligible units, return the fittest.
  selectMate(excludeUnit) {
    const eligible = this.units.filter(u => u !== excludeUnit && u !== this.units[0]);
    if (eligible.length < 1) return null;
    const count = Math.min(3, eligible.length);
    let best = eligible[Math.floor(Math.random() * eligible.length)];
    for (let i = 1; i < count; i++) {
      const candidate = eligible[Math.floor(Math.random() * eligible.length)];
      if (candidate.fitness > best.fitness) best = candidate;
    }
    return best;
  }

  // Sexual reproduction: combine dictionaries from two parents into a child.
  async mate(parentA, parentB) {
    if (this.units.length >= this.maxUnits) return null;

    const genomeA = this._extractGenome(parentA);
    const genomeB = this._extractGenome(parentB);
    const fitnessA = parentA.fitness;
    const fitnessB = parentB.fitness;

    // Build lookup maps.
    const mapA = new Map(genomeA.map(w => [w.name, w.definition]));
    const mapB = new Map(genomeB.map(w => [w.name, w.definition]));

    const childWords = [];
    const added = new Set();

    // Shared words: pick from fitter parent.
    for (const [name, defA] of mapA) {
      if (mapB.has(name)) {
        const def = fitnessA >= fitnessB ? defA : mapB.get(name);
        childWords.push({ name, definition: def });
        added.add(name);
      }
    }

    // Unique to A: SOL-* always, others 50%.
    for (const [name, def] of mapA) {
      if (added.has(name)) continue;
      if (name.startsWith('SOL-') || Math.random() < 0.5) {
        childWords.push({ name, definition: def });
        added.add(name);
      }
    }

    // Unique to B: SOL-* always, others 50%.
    for (const [name, def] of mapB) {
      if (added.has(name)) continue;
      if (name.startsWith('SOL-') || Math.random() < 0.5) {
        childWords.push({ name, definition: def });
        added.add(name);
      }
    }

    // Cap at 50 words.
    childWords.length = Math.min(childWords.length, 50);

    // Spawn a new unit.
    const child = await this._spawn();
    if (!child) return null;

    // Eval the combined dictionary into the child VM.
    for (const w of childWords) {
      child.vm.eval(w.definition);
      if (!child.userWords.includes(w.definition)) child.userWords.push(w.definition);
    }

    child.fitness = 0;
    child.personality = 'hybrid';

    this._emit('mate', {
      parentA: parentA.id, parentB: parentB.id,
      childId: child.id, words: childWords.length
    });

    return child;
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
