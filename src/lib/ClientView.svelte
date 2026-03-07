<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let serverAddr = $state("");
  let { connected = $bindable(false) }: { connected?: boolean } = $props();
  let error = $state("");
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  async function pollStatus() {
    if (!connected) return;
    try {
      const status = await invoke<{ mode: string; error?: string }>("get_status");
      if (status.mode === "idle" && connected) {
        connected = false;
        if (status.error) error = status.error;
      }
    } catch (e) {
      console.error("failed to get status:", e);
    }
  }

  $effect(() => {
    if (connected) {
      pollTimer = setInterval(pollStatus, 1000);
    } else {
      if (pollTimer) clearInterval(pollTimer);
      pollTimer = null;
    }
    return () => {
      if (pollTimer) clearInterval(pollTimer);
    };
  });

  async function toggleConnection() {
    error = "";
    if (connected) {
      try {
        await invoke("stop_client");
        connected = false;
      } catch (e) {
        error = `${e}`;
      }
    } else {
      if (!serverAddr.trim()) {
        error = "Server address is required";
        return;
      }
      try {
        await invoke("start_client", { serverAddr });
        connected = true;
      } catch (e) {
        error = `${e}`;
      }
    }
  }
</script>

<div class="client-view">
  <div class="field">
    <label for="server">Server Address</label>
    <input
      id="server"
      bind:value={serverAddr}
      disabled={connected}
      placeholder="192.168.1.100:4920"
    />
  </div>

  <button
    class="action-btn"
    class:active-danger={connected}
    onclick={toggleConnection}
  >
    {connected ? "Disconnect" : "Connect"}
  </button>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if connected}
    <div class="status-card">
      <div class="status-row">
        <div class="status-dot"></div>
        <span>Connected to {serverAddr}</span>
      </div>
    </div>
  {/if}
</div>

<style>
  .client-view {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .status-card {
    background: var(--bg-secondary);
    border-radius: var(--radius);
    padding: 14px 16px;
  }
</style>
