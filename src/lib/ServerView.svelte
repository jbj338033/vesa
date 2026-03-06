<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import ScreenLayout from "./ScreenLayout.svelte";

  let bindAddr = $state("0.0.0.0:4920");
  let { running = $bindable(false) }: { running?: boolean } = $props();
  let error = $state("");

  let clients: { id: string; name: string; position: string }[] = $state([]);
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  async function pollClients() {
    if (!running) return;
    try {
      clients = await invoke("get_clients");
    } catch {
      // ignore polling errors
    }
  }

  $effect(() => {
    if (running) {
      pollClients();
      pollTimer = setInterval(pollClients, 1000);
    } else {
      if (pollTimer) clearInterval(pollTimer);
      pollTimer = null;
      clients = [];
    }
    return () => {
      if (pollTimer) clearInterval(pollTimer);
    };
  });

  async function toggleServer() {
    error = "";
    if (running) {
      try {
        await invoke("stop_server");
        running = false;
        clients = [];
      } catch (e) {
        error = `${e}`;
      }
    } else {
      try {
        await invoke("start_server", { bindAddr });
        running = true;
      } catch (e) {
        error = `${e}`;
      }
    }
  }

  async function onClientPositionChange(id: string, position: string) {
    clients = clients.map((c) =>
      c.id === id ? { ...c, position } : c
    );
    try {
      await invoke("set_client_position", { position });
    } catch (e) {
      error = `${e}`;
    }
  }
</script>

<div class="server-view">
  <div class="field">
    <label for="bind">Bind Address</label>
    <input
      id="bind"
      bind:value={bindAddr}
      disabled={running}
      placeholder="0.0.0.0:4920"
    />
  </div>

  <button
    class="action-btn"
    class:running
    onclick={toggleServer}
  >
    {running ? "Stop Server" : "Start Server"}
  </button>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if running}
    <div class="layout-section">
      <div class="section-header">
        <div class="status-dot"></div>
        <span>Listening on {bindAddr}</span>
      </div>
      <ScreenLayout {clients} onpositionchange={onClientPositionChange} />
      {#if clients.length === 0}
        <p class="hint">Waiting for clients to connect...</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .server-view {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field label {
    font-size: 12px;
    color: var(--text-secondary);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .action-btn {
    background: var(--accent);
    color: white;
    padding: 12px;
    font-size: 14px;
    width: 100%;
  }

  .action-btn:hover {
    background: var(--accent-hover);
  }

  .action-btn.running {
    background: var(--danger);
  }

  .action-btn.running:hover {
    background: #ff6961;
  }

  .error {
    color: var(--danger);
    font-size: 12px;
    text-align: center;
  }

  .layout-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .section-header {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 500;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--success);
    box-shadow: 0 0 6px rgba(48, 209, 88, 0.4);
    flex-shrink: 0;
  }

  .hint {
    font-size: 12px;
    color: var(--text-secondary);
    text-align: center;
  }
</style>
