import { api, isTauriRuntime, resolveVaultUrl } from "./ipc.js";
import { icon } from "./icons.js";

export function formatDuration(totalSecs) {
  const secs = Math.max(0, Math.floor(Number(totalSecs) || 0));
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

function encodeWav16Mono(samples, sampleRate) {
  const dataLength = samples.length * 2;
  const buffer = new ArrayBuffer(44 + dataLength);
  const view = new DataView(buffer);

  const writeStr = (offset, str) => {
    for (let i = 0; i < str.length; i += 1) {
      view.setUint8(offset + i, str.charCodeAt(i));
    }
  };

  writeStr(0, "RIFF");
  view.setUint32(4, 36 + dataLength, true);
  writeStr(8, "WAVE");
  writeStr(12, "fmt ");
  view.setUint32(16, 16, true); // PCM chunk size
  view.setUint16(20, 1, true); // PCM format
  view.setUint16(22, 1, true); // Mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true); // Byte rate
  view.setUint16(32, 2, true); // Block align
  view.setUint16(34, 16, true); // Bits per sample
  writeStr(36, "data");
  view.setUint32(40, dataLength, true);

  let offset = 44;
  for (let i = 0; i < samples.length; i += 1) {
    const clamped = Math.max(-1, Math.min(1, samples[i]));
    const pcm = clamped < 0 ? clamped * 0x8000 : clamped * 0x7fff;
    view.setInt16(offset, pcm, true);
    offset += 2;
  }

  return new Uint8Array(buffer);
}

function flattenChunks(chunks) {
  let total = 0;
  for (const c of chunks) total += c.length;
  const out = new Float32Array(total);
  let pos = 0;
  for (const c of chunks) {
    out.set(c, pos);
    pos += c.length;
  }
  return out;
}

async function probeAudioDuration(file) {
  return new Promise((resolve) => {
    const url = URL.createObjectURL(file);
    const audio = new Audio();
    let settled = false;
    let timeoutId = null;
    const done = (val) => {
      if (settled) return;
      settled = true;
      if (timeoutId) clearTimeout(timeoutId);
      audio.onloadedmetadata = null;
      audio.onerror = null;
      audio.removeAttribute("src");
      URL.revokeObjectURL(url);
      resolve(val);
    };
    timeoutId = setTimeout(() => done(null), 3000);
    audio.preload = "metadata";
    audio.onloadedmetadata = () => {
      const dur = Number.isFinite(audio.duration) ? audio.duration : null;
      done(dur);
    };
    audio.onerror = () => done(null);
    audio.src = url;
  });
}

export function createVoiceRecorder(onRecorded, onCancel) {
  const card = document.createElement("div");
  card.className = "voice-recorder-card";

  let isRecording = false;
  let isStarting = false;
  let isUploading = false;
  let isDestroyed = false;
  let cancelRequested = false;
  let recordingMode = null;
  let stream = null;
  let audioCtx = null;
  let sourceNode = null;
  let processorNode = null;
  let silenceGain = null;
  let chunks = [];
  let sampleRate = 44100;
  let startTime = 0;
  let timerId = null;

  const header = document.createElement("div");
  header.className = "voice-recorder-header";

  const titleWrap = document.createElement("div");
  titleWrap.className = "voice-recorder-title";
  const micSpan = document.createElement("span");
  micSpan.className = "voice-recorder-icon";
  micSpan.innerHTML = icon("mic", "log-icon");
  const titleLabel = document.createElement("span");
  titleLabel.className = "small-caps-meta";
  titleLabel.textContent = "Voice Note";
  titleWrap.append(micSpan, titleLabel);

  const timerSpan = document.createElement("span");
  timerSpan.className = "waveform-time";
  timerSpan.textContent = "00:00";

  header.append(titleWrap, timerSpan);

  const errorEl = document.createElement("p");
  errorEl.className = "voice-recorder-error";
  errorEl.hidden = true;

  const controls = document.createElement("div");
  controls.className = "voice-recorder-controls";

  const recordBtn = document.createElement("button");
  recordBtn.type = "button";
  recordBtn.className = "btn-editorial small";

  const fileLabel = document.createElement("label");
  fileLabel.className = "btn-editorial small upload-audio-label";
  const uploadIcon = document.createElement("span");
  uploadIcon.innerHTML = icon("upload", "tiny-icon");
  const fileText = document.createElement("span");
  fileText.textContent = "Upload audio file";
  const fileInput = document.createElement("input");
  fileInput.type = "file";
  fileInput.accept = "audio/*";
  fileInput.hidden = true;
  fileLabel.append(uploadIcon, fileText, fileInput);

  const cancelBtn = document.createElement("button");
  cancelBtn.type = "button";
  cancelBtn.className = "btn-editorial small btn-ghost";
  cancelBtn.textContent = "Cancel";

  controls.append(recordBtn, fileLabel, cancelBtn);
  card.append(header, errorEl, controls);

  const setError = (msg) => {
    if (!msg) {
      errorEl.hidden = true;
      errorEl.textContent = "";
    } else {
      errorEl.hidden = false;
      errorEl.textContent = msg;
    }
  };

  const renderRecordButton = () => {
    recordBtn.innerHTML = "";
    recordBtn.classList.toggle("recording-active", isRecording);
    card.classList.toggle("recording-active", isRecording);
    if (isUploading) {
      recordBtn.disabled = true;
      recordBtn.textContent = "Saving note...";
      return;
    }
    if (isStarting) {
      recordBtn.disabled = true;
      recordBtn.textContent = "Starting...";
      return;
    }
    recordBtn.disabled = false;
    if (isRecording) {
      const dot = document.createElement("span");
      dot.className = "recording-pulse-dot";
      dot.style.background = "var(--accent-stamp)";
      const label = document.createElement("span");
      label.textContent = "Stop & Save";
      recordBtn.append(dot, label);
    } else {
      const ic = document.createElement("span");
      ic.innerHTML = icon("mic", "tiny-icon");
      const label = document.createElement("span");
      label.textContent = "Start Recording";
      recordBtn.append(ic, label);
    }
  };

  const teardownAudio = async () => {
    if (timerId) {
      clearInterval(timerId);
      timerId = null;
    }
    if (processorNode) {
      processorNode.onaudioprocess = null;
      try {
        processorNode.disconnect();
      } catch {
        // ignore
      }
      processorNode = null;
    }
    if (sourceNode) {
      try {
        sourceNode.disconnect();
      } catch {
        // ignore
      }
      sourceNode = null;
    }
    if (silenceGain) {
      try {
        silenceGain.disconnect();
      } catch {
        // ignore
      }
      silenceGain = null;
    }
    if (stream) {
      for (const track of stream.getTracks()) {
        track.stop();
      }
      stream = null;
    }
    if (audioCtx) {
      try {
        await audioCtx.close();
      } catch {
        // ignore
      }
      audioCtx = null;
    }
  };

  const startTimer = () => {
    if (timerId) {
      clearInterval(timerId);
    }
    startTime = Date.now();
    timerSpan.textContent = "00:00";
    timerId = setInterval(() => {
      const elapsed = (Date.now() - startTime) / 1000;
      timerSpan.textContent = formatDuration(elapsed);
      if (elapsed >= 1800) {
        stopAndSave();
      }
    }, 200);
  };

  const cancelActiveRecording = async () => {
    cancelRequested = true;
    const mode = recordingMode;
    isRecording = false;
    recordingMode = null;
    chunks = [];
    await teardownAudio();
    if (mode === "native") {
      await api.cancelVoiceRecording().catch(() => {});
    }
  };

  const startRecording = async () => {
    if (isStarting || isRecording || isUploading) return;
    setError("");
    cancelRequested = false;
    isStarting = true;
    renderRecordButton();

    try {
      if (isTauriRuntime()) {
        try {
          await api.startVoiceRecording();
          if (isDestroyed || cancelRequested) {
            await api.cancelVoiceRecording().catch(() => {});
            return;
          }
          recordingMode = "native";
          isRecording = true;
          startTimer();
          return;
        } catch {
          if (isDestroyed || cancelRequested) return;
          // Fall through to WebAudio getUserMedia when native capture is unsupported or unavailable.
        }
      }

      if (!navigator.mediaDevices?.getUserMedia) {
        setError("Microphone capture unavailable here. Use Upload audio file instead.");
        return;
      }

      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      if (isDestroyed || cancelRequested) {
        await teardownAudio();
        return;
      }
      const Ctx = window.AudioContext || window.webkitAudioContext;
      if (!Ctx) {
        throw new Error("WebAudio AudioContext is not supported in this webview.");
      }
      audioCtx = new Ctx();
      sampleRate = audioCtx.sampleRate || 44100;
      chunks = [];
      sourceNode = audioCtx.createMediaStreamSource(stream);
      processorNode = audioCtx.createScriptProcessor(4096, 1, 1);
      silenceGain = audioCtx.createGain();
      silenceGain.gain.value = 0;

      processorNode.onaudioprocess = (ev) => {
        if (!isRecording) return;
        const input = ev.inputBuffer.getChannelData(0);
        chunks.push(new Float32Array(input));
      };

      sourceNode.connect(processorNode);
      processorNode.connect(silenceGain);
      silenceGain.connect(audioCtx.destination);

      recordingMode = "web";
      isRecording = true;
      startTimer();
    } catch (err) {
      recordingMode = null;
      await teardownAudio();
      isRecording = false;
      setError(
        `Microphone unavailable (${err?.message || "permission denied"}). Use Upload audio file.`
      );
    } finally {
      isStarting = false;
      renderRecordButton();
    }
  };

  const stopAndSave = async () => {
    if (!isRecording || isUploading) return;
    const mode = recordingMode;
    isRecording = false;
    recordingMode = null;

    if (timerId) {
      clearInterval(timerId);
      timerId = null;
    }

    if (mode === "native") {
      isUploading = true;
      renderRecordButton();
      try {
        const memo = await api.stopVoiceRecording();
        if (isDestroyed) return;
        isUploading = false;
        renderRecordButton();
        onRecorded?.(memo);
      } catch (err) {
        isUploading = false;
        renderRecordButton();
        setError(err?.message || "Failed to save voice note.");
      }
      return;
    }

    const elapsedSecs = Math.max(0.2, (Date.now() - startTime) / 1000);
    const captured = flattenChunks(chunks);
    const rate = sampleRate;
    chunks = [];
    await teardownAudio();

    if (captured.length === 0) {
      renderRecordButton();
      setError("No audio signal was captured.");
      return;
    }

    isUploading = true;
    renderRecordButton();
    try {
      const wavBytes = encodeWav16Mono(captured, rate);
      const filename = `voice-memo-${Date.now()}.wav`;
      const memo = await api.uploadAudio(wavBytes, filename, elapsedSecs);
      if (isDestroyed) return;
      isUploading = false;
      renderRecordButton();
      onRecorded?.(memo);
    } catch (err) {
      isUploading = false;
      renderRecordButton();
      setError(err?.message || "Failed to save voice note.");
    }
  };

  recordBtn.addEventListener("click", () => {
    if (isUploading || isStarting) return;
    if (isRecording) {
      stopAndSave();
    } else {
      startRecording();
    }
  });

  fileInput.addEventListener("change", async () => {
    const file = fileInput.files?.[0];
    fileInput.value = "";
    if (!file || isUploading) return;
    setError("");
    if (isRecording || isStarting || recordingMode === "native") {
      await cancelActiveRecording();
      renderRecordButton();
    }
    isUploading = true;
    renderRecordButton();
    try {
      const dur = await probeAudioDuration(file);
      const memo = await api.uploadAudio(file, file.name, dur);
      if (isDestroyed) return;
      isUploading = false;
      renderRecordButton();
      onRecorded?.(memo);
    } catch (err) {
      isUploading = false;
      renderRecordButton();
      setError(err?.message || "Failed to upload audio file.");
    }
  });

  cancelBtn.addEventListener("click", async () => {
    await cancelActiveRecording();
    onCancel?.();
  });

  card.destroy = async () => {
    isDestroyed = true;
    await cancelActiveRecording();
  };

  renderRecordButton();
  return card;
}

function normalizePeaks(rawPeaks, count = 64) {
  if (Array.isArray(rawPeaks) && rawPeaks.length > 0) {
    const out = [];
    for (let i = 0; i < count; i += 1) {
      const idx = Math.min(
        rawPeaks.length - 1,
        Math.floor((i / count) * rawPeaks.length)
      );
      const v = Number(rawPeaks[idx]);
      out.push(Number.isFinite(v) ? Math.max(0.12, Math.min(1, v)) : 0.25);
    }
    return out;
  }
  const fallback = [];
  for (let i = 0; i < count; i += 1) {
    const wave = 0.35 + 0.35 * Math.abs(Math.sin(i * 0.37) * Math.cos(i * 0.13));
    fallback.push(wave);
  }
  return fallback;
}

export function createWaveformPlayer(memo, onDelete = null) {
  const ribbon = document.createElement("div");
  ribbon.className = "waveform-ribbon";

  const audio = document.createElement("audio");
  audio.preload = "metadata";
  audio.src = resolveVaultUrl(memo.url);
  audio.hidden = true;

  let isPlaying = false;
  let duration = Number(memo.duration_secs) || 0;

  const playBtn = document.createElement("button");
  playBtn.type = "button";
  playBtn.className = "waveform-play-btn";
  playBtn.setAttribute("aria-label", "Play voice note");
  playBtn.innerHTML = icon("play", "log-icon");

  const body = document.createElement("div");
  body.className = "waveform-body";

  const barsEl = document.createElement("div");
  barsEl.className = "waveform-bars";
  barsEl.tabIndex = 0;
  barsEl.setAttribute("role", "slider");
  barsEl.setAttribute("aria-label", "Audio progress");
  barsEl.setAttribute("aria-valuemin", "0");
  barsEl.setAttribute("aria-valuemax", String(Math.round(duration)));
  barsEl.setAttribute("aria-valuenow", "0");

  const peaks = normalizePeaks(memo.peaks, 64);
  const barNodes = peaks.map((amp) => {
    const bar = document.createElement("div");
    bar.className = "waveform-bar";
    bar.style.height = `${Math.max(16, Math.round(amp * 100))}%`;
    barsEl.appendChild(bar);
    return bar;
  });

  const meta = document.createElement("div");
  meta.className = "waveform-meta";

  const timeEl = document.createElement("span");
  timeEl.className = "waveform-time";
  timeEl.textContent = `00:00 / ${formatDuration(duration)}`;

  const rightMeta = document.createElement("div");
  rightMeta.style.display = "inline-flex";
  rightMeta.style.alignItems = "center";
  rightMeta.style.gap = "0.45rem";

  const fileLabel = document.createElement("span");
  fileLabel.className = "waveform-time";
  fileLabel.textContent = memo.filename || "voice-memo";
  rightMeta.appendChild(fileLabel);

  if (typeof onDelete === "function") {
    const delBtn = document.createElement("button");
    delBtn.type = "button";
    delBtn.className = "topbar-icon-btn";
    delBtn.title = "Remove voice note";
    delBtn.setAttribute("aria-label", "Remove voice note");
    delBtn.innerHTML = icon("trash", "tiny-icon");
    delBtn.addEventListener("click", () => {
      audio.pause();
      onDelete(memo.id);
    });
    rightMeta.appendChild(delBtn);
  }

  meta.append(timeEl, rightMeta);
  body.append(barsEl, meta);

  if (memo.transcript) {
    const tr = document.createElement("p");
    tr.className = "waveform-transcript";
    tr.textContent = memo.transcript;
    body.appendChild(tr);
  }

  ribbon.append(audio, playBtn, body);

  const syncProgress = () => {
    const total =
      Number.isFinite(audio.duration) && audio.duration > 0
        ? audio.duration
        : duration;
    const cur = audio.currentTime || 0;
    const frac = total > 0 ? Math.max(0, Math.min(1, cur / total)) : 0;
    const activeBars = Math.round(frac * barNodes.length);
    barNodes.forEach((b, idx) => {
      b.classList.toggle("played", idx < activeBars);
    });
    barsEl.setAttribute("aria-valuemax", String(Math.round(total)));
    barsEl.setAttribute("aria-valuenow", String(Math.round(cur)));
    timeEl.textContent = `${formatDuration(cur)} / ${formatDuration(total)}`;
  };

  const updatePlayIcon = () => {
    playBtn.innerHTML = icon(isPlaying ? "pause" : "play", "log-icon");
    playBtn.setAttribute(
      "aria-label",
      isPlaying ? "Pause voice note" : "Play voice note"
    );
  };

  playBtn.addEventListener("click", async () => {
    if (isPlaying) {
      audio.pause();
    } else {
      try {
        await audio.play();
      } catch {
        isPlaying = false;
        updatePlayIcon();
      }
    }
  });

  audio.addEventListener("loadedmetadata", () => {
    if (Number.isFinite(audio.duration) && audio.duration > 0) {
      duration = audio.duration;
      syncProgress();
    }
  });
  audio.addEventListener("play", () => {
    isPlaying = true;
    updatePlayIcon();
  });
  audio.addEventListener("pause", () => {
    isPlaying = false;
    updatePlayIcon();
  });
  audio.addEventListener("ended", () => {
    isPlaying = false;
    updatePlayIcon();
    syncProgress();
  });
  audio.addEventListener("timeupdate", syncProgress);

  barsEl.addEventListener("click", (ev) => {
    const rect = barsEl.getBoundingClientRect();
    if (!rect.width) return;
    const frac = Math.max(0, Math.min(1, (ev.clientX - rect.left) / rect.width));
    const total =
      Number.isFinite(audio.duration) && audio.duration > 0
        ? audio.duration
        : duration;
    if (total > 0) {
      audio.currentTime = frac * total;
      syncProgress();
    }
  });

  barsEl.addEventListener("keydown", (ev) => {
    const total =
      Number.isFinite(audio.duration) && audio.duration > 0
        ? audio.duration
        : duration;
    if (total <= 0) return;
    if (ev.key === "ArrowLeft") {
      ev.preventDefault();
      audio.currentTime = Math.max(0, (audio.currentTime || 0) - 5);
      syncProgress();
    } else if (ev.key === "ArrowRight") {
      ev.preventDefault();
      audio.currentTime = Math.min(total, (audio.currentTime || 0) + 5);
      syncProgress();
    }
  });

  ribbon.destroy = () => {
    isPlaying = false;
    audio.pause();
    audio.removeAttribute("src");
    audio.load();
  };

  return ribbon;
}
