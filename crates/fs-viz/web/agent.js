(function () {
  window.SDAgent = {
    init(app) {
      const root = document.getElementById("agent");
      const listEl = document.getElementById("thread-list");
      const tabsEl = document.getElementById("agent-tabs");
      const chatTitleEl = document.getElementById("agent-thread-title");
      const msgsEl = document.getElementById("agent-msgs");
      const chipsEl = document.getElementById("agent-chips");
      const input = document.getElementById("agent-input");
      const modeEl = document.getElementById("agent-mode");
      const modelEl = document.getElementById("agent-model");
      const sendBtn = document.getElementById("agent-send");
      const stopBtn = document.getElementById("agent-stop");
      const warn = document.getElementById("agent-key-warn");
      const toggle = document.getElementById("agent-toggle");
      const strip = document.getElementById("agent-strip");
      const sash = document.getElementById("agent-sash");
      const historyBtn = document.getElementById("agent-history");
      const historyPop = document.getElementById("agent-history-pop");

      const st = {
        agentName: "Agent",
        threads: [],
        current: null,
        attachments: [],
        es: null,
        abort: null,
        streaming: false,
        workspaceSent: false,
        rootsKey: "",
        workspaceMap: [],
        workspaceCwd: "",
        histTimer: null,
        renamingAgent: false,
        renamingThread: false,
        renamingChatTitle: false,
        editingUserMsg: null,
        reverting: false,
        runId: 0,
        streamRunId: 0,
      };

      function persistLayoutChrome() {
        app.state.agentModel = modelEl.value || "composer-2.5";
        const t = st.threads.find((x) => x.id === st.current);
        if (t) {
          t.model = modelEl.value || t.model;
          t.agentMode = currentUiMode();
        }
        app.scheduleSave();
      }

      function currentUiMode() {
        const v = modeEl && modeEl.value;
        return v === "plan" || v === "ask" ? v : "agent";
      }

      function normalizeUiMode(v) {
        return v === "plan" || v === "ask" ? v : "agent";
      }

      function applyThreadChrome(t) {
        if (modelEl) {
          const model = (t && t.model) || app.state.agentModel || "composer-2.5";
          if (model && !Array.prototype.some.call(modelEl.options, (o) => o.value === model)) {
            const opt = document.createElement("option");
            opt.value = model;
            opt.textContent = model;
            modelEl.appendChild(opt);
          }
          modelEl.value = model;
        }
        if (modeEl) modeEl.value = normalizeUiMode(t && t.agentMode);
      }

      function closeAgentPops() {
        if (historyPop) historyPop.hidden = true;
      }

      function toggleAgentPop(pop) {
        if (!pop) return;
        const open = pop.hidden;
        closeAgentPops();
        if (open) pop.hidden = false;
      }

      function cancelEdit() {
        if (!st.editingUserMsg) return;
        st.editingUserMsg = null;
        renderMsgs();
      }

      function threadIsOpen(t) {
        return !!(t && t.open !== false);
      }

      function isEmptyUntitled(t) {
        if (!t) return true;
        const hasChat = (t.messages || []).some(
          (m) => (m.role === "user" || m.role === "assistant") && (m.text || "").trim()
        );
        const titled = !!(t.title && t.title !== "New thread");
        return !hasChat && !titled;
      }

      function selectThread(id) {
        closeAgentPops();
        const t = st.threads.find((x) => x.id === id);
        if (t && t.open === false) {
          t.open = true;
          cancelEdit();
          st.current = id;
          scheduleHistorySave();
          applyThreadChrome(t);
          renderThreads();
          renderMsgs();
          return;
        }
        if (st.current === id) {
          cancelEdit();
          return;
        }
        cancelEdit();
        st.current = id;
        scheduleHistorySave();
        applyThreadChrome(t);
        renderThreads();
        renderMsgs();
      }

      function collectHistory() {
        return {
          agentName: st.agentName || "Agent",
          current: st.current,
          threads: st.threads.map((t) => ({
            id: t.id,
            title: t.title,
            model: t.model || modelEl.value || app.state.agentModel || "composer-2.5",
            agentMode: normalizeUiMode(t.agentMode),
            agentId: t.agentId || undefined,
            local: !!t.local,
            open: t.open !== false,
            messages: (t.messages || [])
              .filter(historyKeep)
              .map((m) => ({
                role: m.role || "assistant",
                text: m.text || "",
              })),
          })),
        };
      }

      function scheduleHistorySave() {
        clearTimeout(st.histTimer);
        st.histTimer = setTimeout(saveHistory, 500);
      }

      async function saveHistory() {
        try {
          await app.api.put("/api/agent-history", collectHistory());
        } catch (_) {}
      }

      async function saveHistoryNow() {
        clearTimeout(st.histTimer);
        st.histTimer = null;
        await saveHistory();
      }

      function setOpen(open) {
        app.state.agentOpen = open;
        root.classList.toggle("collapsed", !open);
        sash.hidden = !open;
        strip.hidden = open;
        document.documentElement.style.setProperty(
          "--agent-w",
          open ? (app.state.agentWidth || 380) + "px" : "0px"
        );
        if (open) root.style.width = (app.state.agentWidth || 380) + "px";
        persistLayoutChrome();
        app.resizeViews();
      }

      function applyKeyWarn(extra) {
        const noKey = !app.state.meta.hasCursorKey;
        if (noKey || extra) {
          warn.hidden = false;
          if (extra) {
            warn.textContent = extra;
          } else {
            warn.innerHTML =
              "Set <code>CURSOR_API_KEY</code> in the environment or a gitignored local env file " +
              "(<code>crates/fs-viz/.env</code>). The cosmos UI still works without the agent sidecar.";
          }
        } else {
          warn.hidden = true;
        }
      }

      function startThreadRename(t, host) {
        if (!t || st.renamingThread) return;
        const sid = String(t.id);
        const esc = typeof CSS !== "undefined" && CSS.escape ? CSS.escape(sid) : sid.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
        const row =
          host ||
          (tabsEl && tabsEl.querySelector('[data-id="' + esc + '"]')) ||
          (listEl && listEl.querySelector('li[data-id="' + esc + '"]'));
        if (!row) return;
        st.renamingThread = true;
        const inp = document.createElement("input");
        inp.value = t.title || "";
        row.textContent = "";
        row.appendChild(inp);
        inp.focus();
        inp.select();
        const done = (commit) => {
          st.renamingThread = false;
          if (commit) {
            t.title = inp.value.trim() || t.title;
            scheduleHistorySave();
            syncThreadTitle(t);
          }
          renderThreads();
        };
        inp.onmousedown = (ev) => ev.stopPropagation();
        inp.onclick = (ev) => ev.stopPropagation();
        inp.ondblclick = (ev) => ev.stopPropagation();
        inp.onblur = () => done(true);
        inp.onkeydown = (ev) => {
          if (ev.key === "Enter") {
            ev.preventDefault();
            inp.blur();
          }
          if (ev.key === "Escape") {
            ev.preventDefault();
            inp.onblur = null;
            done(false);
          }
        };
      }

      async function syncThreadTitle(t) {
        if (!t || t.local || String(t.id).indexOf("local-") === 0) return;
        try {
          await app.api.patch("/api/agent/threads/" + encodeURIComponent(t.id), { title: t.title });
        } catch (_) {}
      }

      function fillThreadRow(el, t, kind) {
        el.classList.toggle("on", t.id === st.current);
        el.dataset.id = t.id;
        el.tabIndex = 0;
        const lab = document.createElement("span");
        lab.className = "t";
        lab.textContent = t.title || "Thread";
        lab.title = "Double-click or F2 to rename";
        const del = document.createElement("button");
        del.type = "button";
        del.className = "del";
        del.title = kind === "history" ? "Delete chat" : "Close chat";
        del.textContent = "×";
        del.onclick = (e) => {
          e.stopPropagation();
          if (kind === "history") deleteThread(t);
          else closeThread(t);
        };
        el.append(lab, del);
        el.onclick = (e) => {
          if (e.target.closest(".del")) return;
          if (e.target.closest("input")) return;
          if (e.detail >= 2) {
            e.preventDefault();
            startThreadRename(t, el);
            return;
          }
          el.focus();
          selectThread(t.id);
        };
        el.onkeydown = (e) => {
          if (e.key === "F2") {
            e.preventDefault();
            e.stopPropagation();
            startThreadRename(t, el);
          }
        };
        lab.ondblclick = (e) => {
          e.preventDefault();
          e.stopPropagation();
          startThreadRename(t, el);
        };
      }

      function renderTabs() {
        if (!tabsEl || st.renamingThread) return;
        tabsEl.innerHTML = "";
        st.threads.filter(threadIsOpen).forEach((t) => {
          const tab = document.createElement("div");
          tab.className = "agent-tab" + (t.id === st.current ? " on" : "");
          tab.setAttribute("role", "tab");
          tab.setAttribute("aria-selected", t.id === st.current ? "true" : "false");
          fillThreadRow(tab, t, "tab");
          tabsEl.appendChild(tab);
        });
      }

      function renderThreads() {
        if (listEl && !st.renamingThread) {
          listEl.innerHTML = "";
          st.threads.filter((t) => !isEmptyUntitled(t)).forEach((t) => {
            const li = document.createElement("li");
            fillThreadRow(li, t, "history");
            listEl.appendChild(li);
          });
        }
        renderTabs();
        renderChatTitle();
      }

      function renderChatTitle() {
        if (!chatTitleEl || st.renamingChatTitle) return;
        const t = st.threads.find((x) => x.id === st.current);
        chatTitleEl.textContent = (t && t.title) || "New thread";
        chatTitleEl.title = "Double-click or F2 to rename";
      }

      function beginChatTitleEdit() {
        if (!chatTitleEl || st.renamingChatTitle || st.renamingThread) return;
        const t = st.threads.find((x) => x.id === st.current);
        if (!t) return;
        st.renamingChatTitle = true;
        const inp = document.createElement("input");
        inp.value = t.title || "";
        chatTitleEl.textContent = "";
        chatTitleEl.appendChild(inp);
        inp.focus();
        inp.select();
        const done = (commit) => {
          st.renamingChatTitle = false;
          if (commit) {
            t.title = inp.value.trim() || t.title;
            scheduleHistorySave();
            syncThreadTitle(t);
          }
          renderThreads();
        };
        inp.onblur = () => done(true);
        inp.onkeydown = (ev) => {
          if (ev.key === "Enter") {
            ev.preventDefault();
            inp.blur();
          }
          if (ev.key === "Escape") {
            ev.preventDefault();
            inp.onblur = null;
            done(false);
          }
        };
      }

      function historyKeep(m) {
        if (!m) return false;
        if (m.role === "action" && m.actionKey === "thinking") return false;
        if (m.role === "assistant" && m.streaming && !(m.text || "").trim()) return false;
        return true;
      }

      function prefixMessages(messages) {
        return (messages || [])
          .filter(historyKeep)
          .filter((m) => (m.role === "user" || m.role === "assistant") && (m.text || "").trim())
          .map((m) => ({ role: m.role, text: m.text }));
      }

      function hasLaterTurns(t, index) {
        const msgs = (t && t.messages) || [];
        for (let i = index + 1; i < msgs.length; i++) {
          const m = msgs[i];
          if (!historyKeep(m)) continue;
          if (m.role === "assistant" && !(m.text || "").trim()) continue;
          return true;
        }
        return false;
      }

      function renderMsgs() {
        if (st.editingUserMsg) return;
        msgsEl.innerHTML = "";
        const t = st.threads.find((x) => x.id === st.current);
        (t && t.messages ? t.messages : []).forEach((m) => {
          if (m.role === "assistant" && !(m.text || "").trim() && (m.streaming || st.streaming)) return;
          const d = document.createElement("div");
          d.className = "msg " + (m.role || "assistant");
          if (m.role === "action" && m.busy) d.classList.add("busy");
          if (m.role === "user") {
            const text = document.createElement("span");
            text.className = "msg-text";
            text.textContent = m.text || "";
            d.appendChild(text);
            d.ondblclick = (e) => {
              e.preventDefault();
              e.stopPropagation();
              startUserEdit(m, d);
            };
          } else {
            d.textContent = m.text || "";
          }
          msgsEl.appendChild(d);
        });
        msgsEl.scrollTop = msgsEl.scrollHeight;
        renderChatTitle();
      }

      function startUserEdit(m, bubble) {
        if (!m || m.role !== "user" || st.editingUserMsg || st.reverting) return;
        st.editingUserMsg = m;
        bubble.innerHTML = "";
        const ta = document.createElement("textarea");
        ta.className = "msg-revert";
        ta.value = m.text || "";
        ta.rows = Math.min(12, Math.max(3, String(ta.value).split("\n").length));
        bubble.appendChild(ta);
        ta.focus();
        ta.select();
        const cancel = () => {
          cancelEdit();
        };
        ta.onkeydown = (ev) => {
          if (ev.key === "Escape") {
            ev.preventDefault();
            ev.stopPropagation();
            cancel();
          } else if (ev.key === "Enter" && !ev.shiftKey) {
            ev.preventDefault();
            ev.stopPropagation();
            commitUserEdit(m, ta.value);
          }
        };
      }

      async function resetSidecarThread(t) {
        if (!t) return;
        const id = t.id;
        try {
          if (id) {
            await app.api.delete("/api/agent/threads/" + encodeURIComponent(id));
          }
        } catch (_) {}
        t.agentId = undefined;
        const model = t.model || modelEl.value || app.state.agentModel || "composer-2.5";
        const body = {
          title: t.title || "New thread",
          model,
          mode: normalizeUiMode(t.agentMode),
          messages: prefixMessages(t.messages),
        };
        if (id) body.id = id;
        try {
          const data = await app.api.post("/api/agent/threads", body);
          applySidecarThread(t, data, model);
          t.agentId = undefined;
        } catch (_) {
          t.agentId = undefined;
        }
        scheduleHistorySave();
        renderThreads();
      }

      async function commitUserEdit(m, raw) {
        const text = String(raw || "").trim();
        st.editingUserMsg = null;
        if (st.reverting) {
          renderMsgs();
          return;
        }
        if (!text) {
          renderMsgs();
          return;
        }
        const t = st.threads.find((x) => x.id === st.current);
        if (!t) {
          renderMsgs();
          return;
        }
        st.reverting = true;
        let shouldSend = false;
        try {
          if (st.streaming) {
            st.runId += 1;
            await stop();
          }
          const idx = (t.messages || []).indexOf(m);
          if (idx < 0) {
            renderMsgs();
            return;
          }
          if (hasLaterTurns(t, idx)) {
            const ok = await app.confirmChoice(
              "Edit message",
              "This discards later messages in this thread and starts a new agent run.",
              "Discard later messages"
            );
            if (!ok) {
              renderMsgs();
              return;
            }
          }
          t.messages = (t.messages || []).slice(0, idx).filter(historyKeep);
          t.agentId = undefined;
          scheduleHistorySave();
          renderThreads();
          renderMsgs();
          await resetSidecarThread(t);
          input.value = text;
          shouldSend = true;
        } finally {
          st.reverting = false;
        }
        if (shouldSend) await send();
      }

      function chipLabel(a) {
        const p = typeof a === "string" ? a : (a && a.path) || "";
        const name = String(p).split(/[/\\]/).pop();
        return name ? "@" + name : "";
      }

      function userMessageText(prompt, attachments) {
        const chips = (attachments || []).map(chipLabel).filter(Boolean);
        if (chips.length && prompt) return chips.join(" ") + "\n" + prompt;
        if (chips.length) return chips.join(" ");
        return prompt || "";
      }

      function insertAction(key, text, busy) {
        const t = st.threads.find((x) => x.id === st.current);
        if (!t) return null;
        t.messages = t.messages || [];
        let m = t.messages.find((x) => x.role === "action" && x.actionKey === key);
        if (!m) {
          m = { role: "action", text: text || "Thinking…", actionKey: key, busy: !!busy };
          t.messages.push(m);
        } else {
          if (text) m.text = text;
          m.busy = !!busy;
        }
        renderMsgs();
        return m;
      }

      function finalizeAssistant(asst, thread) {
        if (!asst) return null;
        delete asst.streaming;
        if (!(asst.text || "").trim() && thread && thread.messages) {
          thread.messages = thread.messages.filter((m) => m !== asst);
        }
        return null;
      }

      function turnShownAssistantText(thread, exclude) {
        const msgs = (thread && thread.messages) || [];
        let start = 0;
        for (let i = msgs.length - 1; i >= 0; i--) {
          if (msgs[i].role === "user") {
            start = i + 1;
            break;
          }
        }
        let out = "";
        for (let i = start; i < msgs.length; i++) {
          const m = msgs[i];
          if (m === exclude) continue;
          if (m.role === "assistant") out += m.text || "";
        }
        return out;
      }

      function lastTurnHasAssistantText(thread) {
        return !!turnShownAssistantText(thread, null).trim();
      }

      function assistantBubbleText(asst, incoming, thread) {
        const shown = turnShownAssistantText(thread, asst);
        const have = (asst && asst.text) || "";
        if (!incoming) return have;
        if (incoming.startsWith(shown + have)) return incoming.slice(shown.length);
        if (incoming.startsWith(shown)) return incoming.slice(shown.length);
        if (!have || incoming.startsWith(have)) return incoming;
        if (have.startsWith(incoming)) return have;
        return incoming;
      }

      function toolActionKey(ev) {
        const id = ev && (ev.call_id || ev.id);
        if (id) return "tool:" + id;
        st.toolCallSeq = (st.toolCallSeq || 0) + 1;
        return "tool:anon-" + st.toolCallSeq;
      }

      function removeThinkingActions() {
        const t = st.threads.find((x) => x.id === st.current);
        if (!t || !t.messages) return;
        t.messages = t.messages.filter((m) => !(m.role === "action" && m.actionKey === "thinking"));
      }

      function finishStreamingMessages() {
        const t = st.threads.find((x) => x.id === st.current);
        if (!t || !t.messages) return;
        t.messages.forEach((m) => {
          if (m.streaming) delete m.streaming;
          if (m.role === "action") m.busy = false;
        });
        t.messages = t.messages.filter(
          (m) => !(m.role === "assistant" && !(m.text || "").trim())
        );
      }

      async function resolveAttachments(list) {
        const out = [];
        for (const a of list || []) {
          const p = typeof a === "string" ? a : a && a.path;
          if (!p) continue;
          const rec = { path: p };
          const buf = app.editor && app.editor.buffer && app.editor.buffer(p);
          if (buf && buf.content != null) {
            rec.content = buf.content;
          } else {
            try {
              const data = await app.api.get("/api/file?path=" + encodeURIComponent(p));
              if (data && data.content != null) rec.content = data.content;
            } catch (_) {}
          }
          out.push(rec);
        }
        return out;
      }

      function renderChips() {
        chipsEl.innerHTML = "";
        st.attachments.forEach((a, i) => {
          const s = document.createElement("span");
          s.className = "chip";
          s.textContent = "@" + String(typeof a === "string" ? a : a.path || "").split(/[/\\]/).pop();
          const x = document.createElement("button");
          x.type = "button";
          x.textContent = "×";
          x.onclick = () => {
            st.attachments.splice(i, 1);
            renderChips();
          };
          s.appendChild(x);
          chipsEl.appendChild(s);
        });
      }

      function addMsg(role, text) {
        const t = st.threads.find((x) => x.id === st.current);
        if (!t) return null;
        const m = { role, text: text || "" };
        t.messages = t.messages || [];
        t.messages.push(m);
        renderMsgs();
        scheduleHistorySave();
        return m;
      }

      async function ensureOpenCurrent() {
        const cur = st.threads.find((x) => x.id === st.current);
        if (threadIsOpen(cur)) return;
        const next = st.threads.find(threadIsOpen);
        if (next) {
          st.current = next.id;
          return;
        }
        await createThread();
      }

      async function closeThread(t) {
        if (!t) return;
        cancelEdit();
        if (isEmptyUntitled(t)) {
          st.threads = st.threads.filter((x) => x.id !== t.id);
        } else {
          t.open = false;
        }
        await ensureOpenCurrent();
        await saveHistoryNow();
        renderThreads();
        renderMsgs();
        applyThreadChrome(st.threads.find((x) => x.id === st.current));
      }

      async function deleteThread(t) {
        if (!t) return;
        cancelEdit();
        try {
          await app.api.post("/api/agent/stop-all", {});
        } catch (_) {}
        try {
          if (!t.local && String(t.id).indexOf("local-") !== 0) {
            await app.api.delete("/api/agent/threads/" + encodeURIComponent(t.id));
          }
        } catch (_) {}
        st.threads = st.threads.filter((x) => x.id !== t.id);
        await ensureOpenCurrent();
        await saveHistoryNow();
        renderThreads();
        renderMsgs();
        applyThreadChrome(st.threads.find((x) => x.id === st.current));
      }

      function applySidecarThread(t, data, model) {
        const id = String((data && (data.id || data.threadId)) || (t && t.id) || "");
        if (!id) throw new Error("Agent sidecar did not create a thread");
        if (t) {
          t.id = id;
          t.local = false;
          t.model = (data && data.model) || t.model || model;
          if (data && data.mode) t.agentMode = normalizeUiMode(data.mode);
          if (data && data.agentId) t.agentId = data.agentId;
          if (data && data.title && (!t.title || t.title === "New thread")) t.title = data.title;
        } else {
          t = {
            id,
            title: (data && data.title) || "New thread",
            local: false,
            open: true,
            messages: [],
            model: (data && data.model) || model,
            agentMode: normalizeUiMode(data && data.mode),
            agentId: (data && data.agentId) || undefined,
          };
          st.threads.unshift(t);
        }
        st.current = id;
        return t;
      }

      async function ensureSidecarThread() {
        let t = st.threads.find((x) => x.id === st.current);
        const model = modelEl.value || (t && t.model) || app.state.agentModel || "composer-2.5";
        const mode = (t && t.agentMode) || currentUiMode();
        const body = {
          model,
          mode,
          title: (t && t.title) || "New thread",
        };
        if (t && t.agentId) body.agentId = t.agentId;
        if (t && t.messages && t.messages.length) {
          body.messages = t.messages
            .filter((m) => (m.role === "user" || m.role === "assistant") && (m.text || "").trim())
            .map((m) => ({ role: m.role, text: m.text }));
        }
        if (t && t.id && !t.local && String(t.id).indexOf("local-") !== 0) body.id = t.id;
        const data = await app.api.post("/api/agent/threads", body);
        t = applySidecarThread(t, data, model);
        scheduleHistorySave();
        renderThreads();
        return t;
      }

      async function createThread() {
        cancelEdit();
        const model = modelEl.value || app.state.agentModel || "composer-2.5";
        const agentMode = currentUiMode();
        let id = "local-" + Date.now();
        let local = true;
        let agentId;
        try {
          const data = await app.api.post("/api/agent/threads", { model, mode: agentMode, title: "New thread" });
          if (data && (data.id || data.threadId)) {
            id = String(data.id || data.threadId);
            local = false;
            if (data.agentId) agentId = data.agentId;
          }
        } catch (e) {
          if (e && e.status === 503) {
            applyKeyWarn("Agent sidecar unavailable (503). Chrome stays up; sending will fail until it starts.");
          }
        }
        const t = { id, title: "New thread", local, open: true, messages: [], model, agentMode, agentId };
        st.threads.unshift(t);
        st.current = id;
        scheduleHistorySave();
        renderThreads();
        renderMsgs();
        return id;
      }

      function normalizeHistory(raw) {
        if (!raw || typeof raw !== "object") {
          return { agentName: "Agent", current: null, threads: [] };
        }
        const hist = raw.history && typeof raw.history === "object" ? raw.history : raw;
        const name = typeof hist.agentName === "string" && hist.agentName.trim() ? hist.agentName.trim() : "Agent";
        const arr = Array.isArray(hist.threads) ? hist.threads : [];
        const threads = arr
          .map((rawT) => {
            const id = String((rawT && (rawT.id || rawT.threadId)) || "");
            if (!id) return null;
            return {
              id,
              title: (rawT && rawT.title) || "Thread",
              local: !!(rawT && rawT.local),
              open: !(rawT && rawT.open === false),
              model: (rawT && rawT.model) || undefined,
              agentMode: normalizeUiMode(rawT && rawT.agentMode),
              agentId: (rawT && rawT.agentId) || undefined,
              messages: Array.isArray(rawT && rawT.messages)
                ? rawT.messages.map((m) => ({
                    role: (m && m.role) || "assistant",
                    text: (m && m.text) || "",
                  }))
                : [],
            };
          })
          .filter(Boolean);
        const current =
          hist.current && threads.some((t) => t.id === hist.current)
            ? hist.current
            : threads[0]
              ? threads[0].id
              : null;
        return { agentName: name, current, threads };
      }

      async function loadHistory() {
        let hist = { agentName: "Agent", current: null, threads: [] };
        try {
          const raw = await app.api.get("/api/agent-history");
          hist = normalizeHistory(raw);
        } catch (_) {}
        if (!hist.threads.length && Array.isArray(app.state.threads) && app.state.threads.length) {
          hist.threads = app.state.threads
            .map((m) => ({
              id: String(m.id || ""),
              title: m.title || "Thread",
              local: !!m.local,
              open: m.open !== false,
              model: m.model,
              agentMode: normalizeUiMode(m.agentMode),
              messages: [],
            }))
            .filter((t) => t.id);
          hist.current = hist.threads[0] ? hist.threads[0].id : null;
        }
        st.agentName = hist.agentName || "Agent";
        st.threads = hist.threads;
        st.current = hist.current;
        const before = st.threads.length;
        const kept = st.threads.filter((t) => !isEmptyUntitled(t));
        if (kept.length) st.threads = kept;
        else if (st.threads.length) {
          st.threads[0].open = true;
          st.threads = [st.threads[0]];
        }
        const prevCurrent = st.current;
        if (!st.threads.length) {
          await createThread();
        } else {
          const openCur = st.threads.find((t) => t.id === st.current && threadIsOpen(t));
          if (!openCur) {
            const next = st.threads.find(threadIsOpen);
            if (next) st.current = next.id;
            else await createThread();
          }
          if (before !== st.threads.length || st.current !== prevCurrent) scheduleHistorySave();
        }
        if (!st.threads.some((t) => t.id === st.current && threadIsOpen(t))) {
          const next = st.threads.find(threadIsOpen);
          if (next) st.current = next.id;
        }
        applyThreadChrome(st.threads.find((x) => x.id === st.current));
        renderThreads();
        renderMsgs();
      }

      function stopStream() {
        if (st.abort) {
          try {
            st.abort.abort();
          } catch (_) {}
          st.abort = null;
        }
        if (st.es) {
          try {
            st.es.close();
          } catch (_) {}
          st.es = null;
        }
        st.streaming = false;
        stopBtn.hidden = true;
        sendBtn.hidden = false;
      }

      function displayToolName(ev) {
        const name = (ev && ev.name) || "tool";
        const args = ev && ev.args;
        if (!args || typeof args !== "object") return name;
        const inner = args.toolName || args.tool_name;
        if (typeof inner !== "string" || !inner) return name;
        if (name === "mcp" || name === "CallMcpTool" || name === "call_mcp_tool" || /mcp/i.test(String(name))) {
          return inner;
        }
        return name;
      }

      function toolActionText(ev) {
        const name = displayToolName(ev);
        const args = ev && ev.args;
        let detail = "";
        if (args && typeof args === "object") {
          if (Array.isArray(args.names) && args.names.length) {
            detail = args.names.map(String).join(", ");
          } else {
            const raw =
              args.path ||
              args.file_path ||
              args.target_file ||
              args.command ||
              args.query ||
              args.pattern ||
              "";
            detail = typeof raw === "string" ? raw : "";
          }
          if (detail.length > 80) detail = detail.slice(0, 77) + "…";
        }
        const fromEv = ev && typeof ev.text === "string" ? ev.text : "";
        if (fromEv) return fromEv;
        const label = detail ? name + " " + detail : name;
        const status = (ev && ev.status) || "running";
        if (status === "error") return label + " (error)";
        if (status === "completed") return label;
        return label + "…";
      }

      function applyStreamEvent(asst, ev, thread) {
        if (st.streamRunId !== st.runId) return { done: true, asst };
        if (!ev) return { done: false, asst };
        if (typeof ev === "string") {
          const msg = asst || addMsg("assistant", "");
          msg.streaming = true;
          msg.text = (msg.text || "") + ev;
          return { done: false, asst: msg };
        }
        if (ev.agentId && thread && !thread.agentId) thread.agentId = ev.agentId;
        if (typeof ev.title === "string" && ev.title.trim() && thread) {
          thread.title = ev.title.trim();
          renderThreads();
          scheduleHistorySave();
        }
        const type = ev.type || "";
        if (type === "done" || type === "cancelled") {
          if (ev.text) {
            const rest = assistantBubbleText(asst, ev.text, thread);
            if (asst) {
              asst.streaming = true;
              asst.text = rest;
              return { done: true, asst };
            }
            if ((rest || "").trim()) {
              const msg = addMsg("assistant", rest);
              return { done: true, asst: msg };
            }
          }
          return { done: true, asst };
        }
        if (type === "startup_error" || type === "run_error") {
          const msg = asst || addMsg("assistant", "");
          msg.role = "err";
          msg.streaming = true;
          msg.text = ev.text || ev.error || "Agent error";
          return { done: true, asst: msg };
        }
        if (type === "thinking" || type === "status") {
          insertAction("thinking", ev.text || "Thinking…", true);
          return { done: false, asst };
        }
        if (type === "tool_call") {
          asst = finalizeAssistant(asst, thread);
          const busy = ev.status !== "completed" && ev.status !== "error";
          insertAction(toolActionKey(ev), toolActionText(ev), busy);
          return { done: false, asst: null };
        }
        const chunk =
          ev.delta ||
          ev.content ||
          (type === "thinking" || type === "tool_call" || type === "status" ? "" : ev.text) ||
          "";
        if (type === "assistant" && ev.text) {
          const msg = asst || addMsg("assistant", "");
          msg.streaming = true;
          msg.text = assistantBubbleText(msg, ev.text, thread);
          return { done: false, asst: msg };
        }
        if (chunk) {
          const msg = asst || addMsg("assistant", "");
          msg.streaming = true;
          msg.text = assistantBubbleText(msg, (msg.text || "") + chunk, thread);
          return { done: false, asst: msg };
        }
        return { done: false, asst };
      }

      async function ensureWorkspace() {
        const roots = [...new Set((app.state.views || []).map((v) => v.root).filter(Boolean))];
        const key = roots.slice().sort().join("\n");
        if (key !== st.rootsKey || !st.workspaceSent) {
          const ws = await app.api.post("/api/workspace", { roots });
          st.rootsKey = key;
          st.workspaceSent = true;
          st.workspaceMap = (ws && ws.map) || [];
          st.workspaceCwd = (ws && ws.cwd) || "";
        }
        if (st.workspaceCwd) {
          try {
            await app.api.post("/api/agent/workspace", { cwd: st.workspaceCwd, map: st.workspaceMap });
          } catch (_) {}
        }
      }

      async function consumeSse(res, asst, thread) {
        const reader = res.body && res.body.getReader ? res.body.getReader() : null;
        if (!reader) {
          const text = await res.text();
          try {
            const ev = JSON.parse(text);
            const next = applyStreamEvent(asst, ev, thread);
            asst = next.asst;
          } catch (_) {
            if (asst) asst.text = text || "(no reply)";
            else if (text) asst = addMsg("assistant", text);
          }
          return asst;
        }
        const dec = new TextDecoder();
        let buf = "";
        for (;;) {
          const chunk = await reader.read();
          if (chunk.done) break;
          buf += dec.decode(chunk.value, { stream: true });
          for (;;) {
            const i = buf.indexOf("\n\n");
            if (i < 0) break;
            const block = buf.slice(0, i);
            buf = buf.slice(i + 2);
            const line = block.split("\n").find((l) => l.indexOf("data:") === 0);
            if (!line) continue;
            const raw = line.slice(5).trim();
            if (!raw) continue;
            try {
              const next = applyStreamEvent(asst, JSON.parse(raw), thread);
              asst = next.asst;
              if (st.streamRunId !== st.runId) return asst;
              renderMsgs();
              scheduleHistorySave();
              if (next.done) return asst;
            } catch (_) {}
          }
        }
        return asst;
      }

      async function send() {
        cancelEdit();
        const prompt = input.value.trim();
        if (!prompt && !st.attachments.length) return;
        if (st.streaming) return;
        const runId = ++st.runId;
        st.streamRunId = runId;
        const still = () => runId === st.runId;
        const pending = st.attachments.slice();
        const userText = userMessageText(prompt, pending);
        st.attachments = [];
        renderChips();
        input.value = "";
        addMsg("user", userText);
        st.toolCallSeq = 0;
        insertAction("thinking", "Thinking…", true);
        st.streaming = true;
        stopBtn.hidden = false;
        sendBtn.hidden = true;
        st.abort = typeof AbortController === "function" ? new AbortController() : null;
        let asst = null;
        try {
          await ensureWorkspace();
        } catch (_) {}
        if (!still()) return;
        try {
          await ensureSidecarThread();
        } catch (e) {
          if (!still()) return;
          applyKeyWarn(
            (e && e.status === 503)
              ? "Agent sidecar unavailable (503). Start fs-viz with Node and CURSOR_API_KEY."
              : app.errText(e)
          );
          removeThinkingActions();
          finishStreamingMessages();
          stopStream();
          renderMsgs();
          scheduleHistorySave();
          return;
        }
        if (!still()) return;
        const t = st.threads.find((x) => x.id === st.current);
        const attachments = await resolveAttachments(pending);
        if (!still()) return;
        if (!st.streaming) {
          removeThinkingActions();
          addMsg("assistant", "(stopped)");
          finishStreamingMessages();
          renderMsgs();
          scheduleHistorySave();
          return;
        }
        const model = modelEl.value || "composer-2.5";
        const agentMode = currentUiMode();
        if (t) {
          t.model = model;
          t.agentMode = agentMode;
        }
        const body = { prompt, attachments, model, mode: agentMode };
        const url = "/api/agent/threads/" + encodeURIComponent(st.current) + "/stream";
        try {
          const res = await fetch(url, {
            method: "POST",
            headers: { "Content-Type": "application/json", Accept: "text/event-stream" },
            body: JSON.stringify(body),
            signal: st.abort ? st.abort.signal : undefined,
          });
          if (!still()) return;
          const ct = (res.headers.get("content-type") || "").toLowerCase();
          if (res.status === 404) {
            if (t) t.local = true;
            try {
              await ensureSidecarThread();
            } catch (_) {}
            if (!still()) return;
            const fallback = await app.api.post(
              "/api/agent/threads/" + encodeURIComponent(st.current) + "/send",
              body
            );
            if (!still()) return;
            if (fallback && fallback.agentId && t) t.agentId = fallback.agentId;
            if (fallback && fallback.title && t && (!t.title || t.title === "New thread")) {
              t.title = fallback.title;
              renderThreads();
              scheduleHistorySave();
            }
            asst = addMsg("assistant", "");
            if (fallback && fallback.ok === false) {
              asst.role = "err";
              asst.text = fallback.error || fallback.text || "agent error";
            } else {
              asst.text = (fallback && (fallback.text || fallback.reply || fallback.message)) || "(no reply)";
            }
          } else if (!res.ok && !ct.includes("event-stream")) {
            let data = null;
            try {
              data = await res.json();
            } catch (_) {}
            if (!still()) return;
            asst = addMsg("assistant", "");
            asst.role = "err";
            asst.text = (data && (data.error || data.text)) || "agent error (" + res.status + ")";
            if (res.status === 400 || (data && /CURSOR_API_KEY/.test(String(asst.text)))) {
              app.state.meta.hasCursorKey = false;
              applyKeyWarn();
            }
          } else if (ct.includes("event-stream") || res.ok) {
            asst = await consumeSse(res, asst, t);
            if (!still()) return;
            if ((!asst || !(asst.text || "").trim()) && !lastTurnHasAssistantText(t)) {
              asst = asst || addMsg("assistant", "");
              asst.text = "(no reply)";
            }
          }
        } catch (e) {
          if (!still()) return;
          if (e && e.name === "AbortError") {
            if (asst && !asst.text) asst.text = "(stopped)";
            else if (!asst && !lastTurnHasAssistantText(t)) addMsg("assistant", "(stopped)");
          } else {
            asst = asst || addMsg("assistant", "");
            asst.role = "err";
            asst.text = app.errText(e);
          }
        }
        if (!still()) return;
        removeThinkingActions();
        finishStreamingMessages();
        renderMsgs();
        scheduleHistorySave();
        stopStream();
      }

      function focusComposer() {
        if (!app.state.agentOpen) setOpen(true);
        setTimeout(() => input.focus(), 0);
      }

      function newThread() {
        cancelEdit();
        if (!app.state.agentOpen) setOpen(true);
        return createThread();
      }

      function renameFocusedThread() {
        const active = document.activeElement;
        if (active && active.closest && active.closest("#agent-thread-title")) {
          beginChatTitleEdit();
          return;
        }
        const tab = active && active.closest ? active.closest("#agent-tabs [data-id]") : null;
        const li = active && active.closest ? active.closest("#thread-list li") : null;
        const host = tab || li;
        const id = host && host.dataset.id;
        const t =
          (id && st.threads.find((x) => x.id === id)) ||
          st.threads.find((x) => x.id === st.current);
        if (t) startThreadRename(t, host);
      }

      async function stop() {
        try {
          await app.api.post("/api/agent/stop-all", {});
        } catch (_) {}
        stopStream();
        removeThinkingActions();
        finishStreamingMessages();
        renderMsgs();
        scheduleHistorySave();
      }

      sendBtn.onclick = send;
      stopBtn.onclick = () => {
        stop();
      };
      input.addEventListener("keydown", (e) => {
        if (e.key === "Enter" && !e.shiftKey) {
          e.preventDefault();
          send();
        }
      });
      const models = ["composer-2.5", "grok-4.6", "gpt-5.1"];
      const savedModel = app.state.agentModel || "composer-2.5";
      if (models.indexOf(savedModel) < 0) {
        const opt = document.createElement("option");
        opt.value = savedModel;
        opt.textContent = savedModel;
        modelEl.appendChild(opt);
      }
      modelEl.value = savedModel;
      modelEl.addEventListener("change", () => {
        cancelEdit();
        persistLayoutChrome();
        scheduleHistorySave();
      });
      if (modeEl) {
        modeEl.addEventListener("change", () => {
          cancelEdit();
          persistLayoutChrome();
          scheduleHistorySave();
        });
      }
      document.getElementById("agent-new").onclick = () => newThread();
      toggle.onclick = () => setOpen(false);
      strip.onclick = () => setOpen(true);

      if (historyBtn && historyPop) {
        historyBtn.addEventListener("click", (e) => {
          e.stopPropagation();
          toggleAgentPop(historyPop);
        });
      }
      document.addEventListener("mousedown", (e) => {
        if (historyPop && !historyPop.hidden) {
          if (!historyPop.contains(e.target) && historyBtn && !historyBtn.contains(e.target)) {
            historyPop.hidden = true;
          }
        }
      });
      document.addEventListener("keydown", (e) => {
        if (e.key === "Escape") closeAgentPops();
      });
      document.addEventListener(
        "mousedown",
        (e) => {
          if (!st.editingUserMsg) return;
          if (e.target.closest && e.target.closest("textarea.msg-revert")) return;
          if (e.target.closest && e.target.closest("#modal")) return;
          cancelEdit();
        },
        true
      );

      if (chatTitleEl) {
        chatTitleEl.tabIndex = 0;
        chatTitleEl.title = "Double-click or F2 to rename";
        chatTitleEl.addEventListener("dblclick", (e) => {
          e.preventDefault();
          e.stopPropagation();
          beginChatTitleEdit();
        });
        chatTitleEl.addEventListener("keydown", (e) => {
          if (e.key === "F2") {
            e.preventDefault();
            e.stopPropagation();
            beginChatTitleEdit();
          }
        });
      }

      sash.addEventListener("pointerdown", (e) => {
        const start = e.clientX;
        const ow = app.state.agentWidth || root.getBoundingClientRect().width;
        function mv(ev) {
          const dx = start - ev.clientX;
          const w = Math.max(240, Math.min(640, ow + dx));
          app.state.agentWidth = w;
          root.style.width = w + "px";
          document.documentElement.style.setProperty("--agent-w", w + "px");
          app.resizeViews();
        }
        function up() {
          window.removeEventListener("pointermove", mv);
          window.removeEventListener("pointerup", up);
          persistLayoutChrome();
        }
        window.addEventListener("pointermove", mv);
        window.addEventListener("pointerup", up);
      });

      root.style.width = (app.state.agentWidth || 380) + "px";
      setOpen(app.state.agentOpen !== false);
      applyKeyWarn();

      (async () => {
        try {
          const health = await app.api.get("/api/agent/health");
          if (health && health.hasKey === false) {
            app.state.meta.hasCursorKey = false;
            applyKeyWarn();
          }
        } catch (e) {
          if (e && (e.status === 503 || e.status === 0)) {
            applyKeyWarn(
              "Agent sidecar unavailable. Set CURSOR_API_KEY and keep the sidecar running to chat. Cosmos still works."
            );
          }
        }
        await loadHistory();
      })();

      return {
        attach(paths) {
          (paths || []).forEach((p) => {
            if (p && !st.attachments.some((x) => (typeof x === "string" ? x : x.path) === p)) {
              st.attachments.push({ path: p });
            }
          });
          renderChips();
          if (!app.state.agentOpen) setOpen(true);
          setTimeout(() => input.focus(), 0);
        },
        setOpen,
        focusComposer,
        newThread,
        stop,
        applyKeyWarn,
        renameFocusedThread,
        serialize() {
          return collectHistory();
        },
      };
    },
  };
})();
