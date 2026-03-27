// unit.js — WASM glue + WebSocket mesh client for the unit Forth nanobot

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
      env: {}
    });
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

// ---------------------------------------------------------------------------
// WebSocket mesh client
// ---------------------------------------------------------------------------

class MeshClient {
  constructor(onMessage, onStatusChange) {
    this.ws = null;
    this.url = null;
    this.onMessage = onMessage;
    this.onStatusChange = onStatusChange;
    this.reconnectTimer = null;
    this.reconnectDelay = 1000;
    this.maxReconnectDelay = 30000;
    this.heartbeatTimer = null;
    this.peers = 0;
    this.browsers = 0;
    this.fitness = 0;
  }

  connect(url) {
    this.url = url;
    this.reconnectDelay = 1000;
    this._connect();
  }

  _connect() {
    if (this.ws && this.ws.readyState <= 1) {
      this.ws.close();
    }

    try {
      this.ws = new WebSocket(this.url);
    } catch (e) {
      this.onStatusChange('error', e.message);
      this._scheduleReconnect();
      return;
    }

    this.ws.onopen = () => {
      console.log('[mesh] WebSocket connected');
      this.reconnectDelay = 1000;
      this.onStatusChange('connected', null);
      this._startHeartbeat();
    };

    this.ws.onmessage = (event) => {
      console.log('[mesh] message:', event.data.substring(0, 100));
      try {
        const msg = JSON.parse(event.data);
        if (msg.type === 'mesh_state') {
          this.peers = msg.peers || 0;
          this.browsers = msg.browsers || 0;
          this.fitness = msg.fitness || 0;
          this.onStatusChange('connected', null);
        }
        this.onMessage(msg);
      } catch (e) {
        // Ignore parse errors.
      }
    };

    this.ws.onclose = (e) => {
      console.log('[mesh] WebSocket closed:', e.code, e.reason);
      this._stopHeartbeat();
      this.onStatusChange('disconnected', null);
      this._scheduleReconnect();
    };

    this.ws.onerror = (e) => {
      console.error('[mesh] WebSocket error:', e);
      this.onStatusChange('error', 'ws:// blocked — use Firefox or Chrome --disable-features=PrivateNetworkAccessRespectPreflightResults');
    };
  }

  disconnect() {
    this.url = null;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this._stopHeartbeat();
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    this.onStatusChange('disconnected', null);
  }

  send(msg) {
    if (this.ws && this.ws.readyState === 1) {
      this.ws.send(JSON.stringify(msg));
    }
  }

  submitGoal(code, priority) {
    this.send({ type: 'goal_submit', code, priority: priority || 5 });
  }

  isConnected() {
    return this.ws && this.ws.readyState === 1;
  }

  _startHeartbeat() {
    this._stopHeartbeat();
    this.heartbeatTimer = setInterval(() => {
      this.send({ type: 'heartbeat', fitness: this.fitness });
    }, 2000);
  }

  _stopHeartbeat() {
    if (this.heartbeatTimer) {
      clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
  }

  _scheduleReconnect() {
    if (!this.url) return;
    this.reconnectTimer = setTimeout(() => {
      this._connect();
    }, this.reconnectDelay);
    this.reconnectDelay = Math.min(this.reconnectDelay * 1.5, this.maxReconnectDelay);
  }
}
