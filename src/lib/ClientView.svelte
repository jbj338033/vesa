<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let serverAddr = $state("");
  let { connected = $bindable(false) }: { connected?: boolean } = $props();
  let error = $state("");

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
    class:connected
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

  .action-btn.connected {
    background: var(--danger);
  }

  .action-btn.connected:hover {
    background: #ff6961;
  }

  .error {
    color: var(--danger);
    font-size: 12px;
    text-align: center;
  }

  .status-card {
    background: var(--bg-secondary);
    border-radius: var(--radius);
    padding: 14px 16px;
  }

  .status-row {
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
</style>
