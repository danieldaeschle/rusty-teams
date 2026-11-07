(() => {
  if (window.__rustyCallShadow || !window.RTCPeerConnection || !window.chrome?.webview) {
    return;
  }
  window.__rustyCallShadow = true;
  const STATS_INTERVAL_MS = 2000;
  const SIGNALING_HOST = /(^|\.)skype\.com$/;
  const post = (event, data) => {
    try {
      window.chrome.webview.postMessage(JSON.stringify({ kind: "callShadow", at: Date.now(), event, data }));
    } catch (_) {}
  };
  const sdpSummary = (sdp) =>
    (sdp || "")
      .split(/\r?\n/)
      .filter((line) => /^(m=|a=rtpmap:|a=mid:|a=(sendrecv|sendonly|recvonly|inactive)$)/.test(line))
      .map((line) => line.replace(/^m=(\w+) \d+/, "m=$1"));
  const statsSummary = (report) => {
    const rows = [];
    const codecs = {};
    report.forEach((entry) => {
      if (entry.type === "codec") {
        codecs[entry.id] = entry.mimeType;
      }
    });
    report.forEach((entry) => {
      if (entry.type === "inbound-rtp" || entry.type === "outbound-rtp") {
        rows.push({
          type: entry.type,
          kind: entry.kind,
          mid: entry.mid,
          codec: codecs[entry.codecId],
          bytes: entry.bytesReceived ?? entry.bytesSent,
          packets: entry.packetsReceived ?? entry.packetsSent,
          packetsLost: entry.packetsLost,
          jitter: entry.jitter,
          framesPerSecond: entry.framesPerSecond,
          frameWidth: entry.frameWidth,
          frameHeight: entry.frameHeight,
          qualityLimitationReason: entry.qualityLimitationReason,
          concealedSamples: entry.concealedSamples,
        });
      }
      if (entry.type === "candidate-pair" && entry.nominated && entry.state === "succeeded") {
        const local = report.get(entry.localCandidateId);
        const remote = report.get(entry.remoteCandidateId);
        rows.push({
          type: "pair",
          roundTripTime: entry.currentRoundTripTime,
          availableOutgoingBitrate: entry.availableOutgoingBitrate,
          local: local && `${local.candidateType}/${local.protocol}`,
          remote: remote && `${remote.candidateType}/${remote.protocol}`,
        });
      }
    });
    return rows;
  };
  let nextPeer = 0;
  const Native = window.RTCPeerConnection;
  class ShadowPeer extends Native {
    constructor(configuration, ...rest) {
      super(configuration, ...rest);
      const peer = ++nextPeer;
      this.__shadowPeer = peer;
      post("peer", {
        peer,
        bundlePolicy: configuration?.bundlePolicy,
        iceTransportPolicy: configuration?.iceTransportPolicy,
        iceServers: (configuration?.iceServers || []).flatMap((server) =>
          [].concat(server.urls || []).map((url) => String(url).split("?")[0])
        ),
      });
      this.addEventListener("iceconnectionstatechange", () => post("ice", { peer, state: this.iceConnectionState }));
      this.addEventListener("connectionstatechange", () => post("connection", { peer, state: this.connectionState }));
      this.addEventListener("icegatheringstatechange", () => post("gathering", { peer, state: this.iceGatheringState }));
      this.addEventListener("icecandidate", (event) => {
        if (event.candidate) {
          post("candidate", { peer, type: event.candidate.type, protocol: event.candidate.protocol });
        }
      });
      this.addEventListener("track", (event) => post("track", { peer, kind: event.track.kind, mid: event.transceiver?.mid }));
      const timer = setInterval(() => {
        if (this.connectionState === "closed") {
          clearInterval(timer);
          return;
        }
        this.getStats()
          .then((report) => post("stats", { peer, rows: statsSummary(report) }))
          .catch(() => {});
      }, STATS_INTERVAL_MS);
    }
    async setLocalDescription(...args) {
      const result = await super.setLocalDescription(...args);
      post("local", { peer: this.__shadowPeer, type: this.localDescription?.type, lines: sdpSummary(this.localDescription?.sdp) });
      return result;
    }
    async setRemoteDescription(...args) {
      const result = await super.setRemoteDescription(...args);
      post("remote", { peer: this.__shadowPeer, type: this.remoteDescription?.type, lines: sdpSummary(this.remoteDescription?.sdp) });
      return result;
    }
    close() {
      post("close", { peer: this.__shadowPeer });
      return super.close();
    }
  }
  window.RTCPeerConnection = ShadowPeer;
  const nativeFetch = window.fetch;
  window.fetch = function (input, init) {
    const started = performance.now();
    let target;
    try {
      target = new URL(input instanceof Request ? input.url : String(input), location.href);
    } catch (_) {}
    const watched = target && SIGNALING_HOST.test(target.hostname);
    const promise = nativeFetch.apply(this, arguments);
    if (watched) {
      const method = (init?.method || input?.method || "GET").toUpperCase();
      const path = target.hostname + target.pathname.replace(/[0-9a-f-]{16,}|[A-Za-z0-9_-]{32,}/g, "{id}");
      promise.then(
        (response) => post("request", { method, path, status: response.status, ms: Math.round(performance.now() - started) }),
        () => post("request", { method, path, status: 0, ms: Math.round(performance.now() - started) })
      );
    }
    return promise;
  };
})();
