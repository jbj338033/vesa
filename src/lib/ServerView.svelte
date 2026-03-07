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
      const status = await invoke<{ mode: string; error?: string }>("get_status");
      if (status.mode === "idle" && running) {
        running = false;
        clients = [];
        if (status.error) error = status.error;
        return;
      }
    } catch (e) {
      console.error("failed to get status:", e);
    }
    try {
      clients = await invoke("get_clients");
    } catch (e) {
      console.error("failed to get clients:", e);
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
    class:active-danger={running}
    onclick={toggleServer}
  >
    {running ? "Stop Server" : "Start Server"}
  </button>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if running}
    <div class="layout-section">
      <div class="status-row">
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

  .layout-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .hint {
    font-size: 12px;
    color: var(--text-secondary);
    text-align: center;
  }
</style>
