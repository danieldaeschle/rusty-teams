(() => {
  const VERSION = 3;
  const GLOBAL_NAME = '__chatsvcTrouter';
  const BINDING_NAME = '__chatsvcRealtime';
  const EPID_KEY = '__chatsvcEpid';
  const REGISTRAR = 'https://teams.cloud.microsoft/registrar/prod/V2/registrations';
  const UI_VERSION = '1415/26091712213';
  const PING_MS = 30000;
  const SILENCE_LIMIT_MS = 100000;
  const REREGISTER_MS = 40 * 60 * 1000;
  const CONNECT_TIMEOUT_MS = 20000;
  const MAX_BACKOFF_MS = 30000;
  const MAX_QUEUE = 200;
  const MAX_PRESENCE_ENTRIES = 1000;
  const MAX_PRESENCE_FIELD = 128;
  const MAX_SENDER_NAME = 256;

  if (window.top !== window || !location.origin.startsWith('https://teams.')) return;
  const existing = window[GLOBAL_NAME];
  if (existing && existing.version === VERSION) return;
  if (existing) existing.shutdown();

  const loadEpid = () => {
    try {
      const stored = sessionStorage.getItem(EPID_KEY);
      if (stored) return stored;
      const created = crypto.randomUUID();
      sessionStorage.setItem(EPID_KEY, created);
      return created;
    } catch (error) {
      return crypto.randomUUID();
    }
  };

  const state = {
    token: null, host: null, epid: loadEpid(), socket: null, ready: false,
    closed: false, ackCounter: 0, lastFrameAt: 0, registeredAt: 0, retry: 0, reconnectTimer: null, queue: [],
  };

  const flush = () => {
    const sink = window[BINDING_NAME];
    if (typeof sink !== 'function') return;
    while (state.queue.length) {
      try { sink(JSON.stringify(state.queue[0])); } catch (error) { return; }
      state.queue.shift();
    }
  };
  const forward = (message) => {
    state.queue.push(message);
    if (state.queue.length > MAX_QUEUE) state.queue.shift();
    flush();
  };
  const status = (kind, detail) => forward({channel: 'status', kind, detail: detail || ''});
  const announceEndpoint = () => forward({channel: 'endpoint', endpointId: state.epid, trouterUri: state.surl + '/unifiedPresenceService'});

  const conversationOf = (resource) => {
    const link = (resource && (resource.conversationLink || resource.to || resource.id)) || '';
    const match = /conversations\/([^;\/?]+)/.exec(String(link));
    const raw = match ? match[1] : String(link).split(';')[0];
    try { return decodeURIComponent(raw); } catch (error) { return raw; }
  };
  const classify = (resourceType, messageType) => {
    if (resourceType === 'NewMessage') {
      if (/^Control\/(Clear)?Typing/.test(messageType)) return 'typing';
      if (messageType === 'ThreadActivity/MemberConsumptionHorizonUpdate') return 'read_receipt';
      if (/^ThreadActivity\//.test(messageType)) return 'thread_activity';
      return /^Control\//.test(messageType) ? 'control' : 'new_message';
    }
    if (resourceType === 'MessageUpdate') return 'message_update';
    if (resourceType === 'ThreadUpdate') return 'thread_update';
    return 'other';
  };
  const typingDetails = (resource) => {
    const sender = /\/contacts\/8:orgid:([0-9a-f-]{1,64})\/?$/i.exec(String((resource && resource.from) || ''));
    const name = resource && resource.imdisplayname;
    return {
      typing: /^Control\/ClearTyping/.test(String((resource && resource.messagetype) || '')) ? 'clear' : 'start',
      senderId: sender ? sender[1] : null,
      senderName: typeof name === 'string' ? name.slice(0, MAX_SENDER_NAME) : null,
    };
  };
  const forwardNotification = (path, body) => {
    const receivedAt = Date.now();
    if (path === 'unifiedPresenceService') {
      const cut = (value) => (typeof value === 'string' ? value.slice(0, MAX_PRESENCE_FIELD) : null);
      const entries = ((body && Array.isArray(body.presence)) ? body.presence : []).slice(0, MAX_PRESENCE_ENTRIES)
        .map((entry) => ({mri: cut(entry && entry.mri), availability: cut(entry && entry.presence && entry.presence.availability),
          activity: cut(entry && entry.presence && entry.presence.activity)}))
        .filter((entry) => entry.mri && entry.availability);
      if (entries.length) forward({channel: 'presence', entries});
      return;
    }
    const resource = body && body.resource;
    const resourceType = String((body && body.resourceType) || path || 'unknown').slice(0, 64);
    const eventKind = path === 'messaging' ? classify(resourceType, String((resource && resource.messagetype) || '')) : 'other';
    const messageLike = eventKind === 'new_message' || eventKind === 'message_update';
    forward({
      channel: 'event', resourceType, eventKind, receivedAt,
      conversationId: resource ? conversationOf(resource) || null : null,
      messageId: messageLike && resource.id ? String(resource.id) : null,
      ...(eventKind === 'typing' ? typingDetails(resource) : {}),
    });
  };

  const parseFrame = (data) => {
    const match = /^(\d):([^:]*):([^:]*)(?::([\s\S]*))?$/.exec(data);
    return match ? {type: match[1], data: match[4] || ''} : null;
  };
  const send = (text) => { if (state.socket && state.socket.readyState === 1) state.socket.send(text); };
  const sendEvent = (name, args) => send(`5:${++state.ackCounter}+::` + JSON.stringify({name, args}));
  const activity = () => sendEvent('user.activity', [{state: 'active', cv: crypto.randomUUID()}]);

  const register = async (surl) => {
    state.surl = surl || state.surl;
    const body = {
      clientDescription: {appId: 'TeamsCDLWebWorker', aesKey: '', languageId: 'en-US', platform: 'chrome',
        templateKey: 'TeamsCDLWebWorker_2.6', platformUIVersion: UI_VERSION},
      registrationId: state.epid, nodeId: '',
      transports: {TROUTER: [{context: '', path: state.surl, ttl: 3600}]},
    };
    const response = await fetch(REGISTRAR, {
      method: 'POST', body: JSON.stringify(body),
      headers: {Authorization: 'Bearer ' + state.token, 'X-MS-Migration': 'True',
        'Content-Type': 'application/json', Accept: 'application/json, text/javascript'},
    });
    if (!response.ok) throw new Error('registrar_http_' + response.status);
    state.registeredAt = Date.now();
  };
  const registerDetail = (error) => (/^registrar_http_\d+$/.test(error && error.message) ? error.message : 'registrar_failed');

  const socketUrl = () => {
    const clientInfo = JSON.stringify({cv: '2026.36.01.1', ua: 'TeamsCDL', hr: '', v: UI_VERSION});
    return `wss://${state.host}/v4/c?tc=${encodeURIComponent(clientInfo)}&timeout=40&epid=${state.epid}`
      + `&ccid=&cor_id=${crypto.randomUUID()}&con_num=${Date.now()}_0`;
  };

  const scheduleReconnect = () => {
    if (state.closed || state.reconnectTimer) return;
    state.retry++;
    const delay = Math.min(MAX_BACKOFF_MS, 1000 * 2 ** state.retry);
    state.reconnectTimer = setTimeout(() => { state.reconnectTimer = null; connect(); }, delay);
  };

  const onFrame = async (socket, message) => {
    state.lastFrameAt = Date.now();
    const packet = parseFrame(String(message.data));
    if (!packet) return;
    if (packet.type === '1') {
      send('5:::' + JSON.stringify({name: 'user.authenticate',
        args: [{headers: {'X-Ms-Test-User': 'False', Authorization: 'Bearer ' + state.token, 'X-MS-Migration': 'True'}}]}));
    } else if (packet.type === '2') {
      send('2::');
    } else if (packet.type === '5') {
      let event;
      try { event = JSON.parse(packet.data); } catch (error) { return; }
      if (event.name === 'trouter.connected') {
        try {
          await register(event.args[0].surl);
          activity();
          state.ready = true;
          state.retry = 0;
          announceEndpoint();
          status('connected');
        } catch (error) {
          status('error', registerDetail(error));
          socket.close();
        }
      } else if (event.name === 'trouter.message_loss') {
        sendEvent('trouter.processed_message_loss', event.args);
        const dropped = (event.args && event.args[0] && event.args[0].droppedIndicators) || [];
        status('message_loss', dropped.map((indicator) => String(indicator.tag)).join(',').slice(0, 200));
      }
    } else if (packet.type === '3') {
      let request;
      try { request = JSON.parse(packet.data); } catch (error) { return; }
      send('3:::' + JSON.stringify({id: request.id, status: 200, headers: {}, body: ''}));
      let body = null;
      try { body = JSON.parse(request.body); } catch (error) {}
      forwardNotification(String(request.url || '').replace(/^\/v4\/f\/[^/]+\//, ''), body);
    }
  };

  const connect = () => {
    if (state.closed || state.socket || !state.token || !state.host) return;
    const socket = state.socket = new WebSocket(socketUrl());
    state.ready = false;
    state.lastFrameAt = Date.now();
    const timer = setTimeout(() => { if (!state.ready) socket.close(); }, CONNECT_TIMEOUT_MS);
    socket.onmessage = (message) => { onFrame(socket, message); };
    socket.onerror = () => {};
    socket.onclose = () => {
      clearTimeout(timer);
      if (state.socket !== socket) return;
      const wasReady = state.ready;
      state.socket = null;
      state.ready = false;
      if (!state.closed) status('disconnected', wasReady ? 'closed' : 'connect_failed');
      scheduleReconnect();
    };
  };

  const maintain = setInterval(() => {
    if (!state.socket || state.socket.readyState !== 1) return;
    if (Date.now() - state.lastFrameAt > SILENCE_LIMIT_MS) { state.socket.close(); return; }
    sendEvent('ping', []);
    activity();
    if (state.ready && Date.now() - state.registeredAt > REREGISTER_MS) {
      register().catch((error) => status('error', registerDetail(error)));
    }
  }, PING_MS);

  const shutdown = () => {
    state.closed = true;
    clearInterval(maintain);
    clearTimeout(state.reconnectTimer);
    if (state.socket) state.socket.close();
    if (window[GLOBAL_NAME] === api) delete window[GLOBAL_NAME];
  };

  const api = {
    version: VERSION,
    ensure: (config) => {
      state.token = config.token;
      state.host = config.host;
      connect();
      if (state.ready) {
        announceEndpoint();
        status('connected', 'reattached');
      }
      flush();
      return state.ready ? 'ready' : state.socket ? 'connecting' : 'waiting';
    },
    shutdown,
    stop: async () => {
      let unregistered = null;
      if (state.token) {
        try {
          const response = await fetch(`${REGISTRAR}/${state.epid}`, {
            method: 'DELETE', headers: {Authorization: 'Bearer ' + state.token, 'X-MS-Migration': 'True'}});
          unregistered = response.status;
        } catch (error) {}
      }
      shutdown();
      try { sessionStorage.removeItem(EPID_KEY); } catch (error) {}
      return {unregistered};
    },
  };
  window[GLOBAL_NAME] = api;
})();
