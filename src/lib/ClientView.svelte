<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let serverAddr = $state("");
  let position = $state("Right");
  let connected = $state(false);
  let status = $state("Disconnected");

  async function toggleConnection() {
    if (connected) {
      await invoke("stop_client");
      connected = false;
      status = "Disconnected";
    } else {
      try {
        await invoke("start_client", { serverAddr, position });
        connected = true;
        status = `Connected to ${serverAddr}`;
      } catch (e) {
        status = `Failed: ${e}`;
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

  <div class="field">
    <label for="position">Screen Position</label>
    <select id="position" bind:value={position} disabled={connected}>
      <option value="Left">Left</option>
      <option value="Right">Right</option>
      <option value="Top">Top</option>
      <option value="Bottom">Bottom</option>
    </select>
  </div>

  <button class="toggle-btn" class:connected onclick={toggleConnection}>
    {connected ? "Disconnect" : "Connect"}
  </button>

  <p class="status">{status}</p>
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
    font-size: 13px;
    color: var(--text-secondary);
    font-weight: 500;
  }

  select {
    appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%23a0a0a0' d='M6 8L1 3h10z'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 12px center;
    padding-right: 32px;
  }

  .toggle-btn {
    background: var(--accent);
    color: white;
    padding: 10px;
    font-size: 15px;
  }

  .toggle-btn:hover {
    background: var(--accent-hover);
  }

  .toggle-btn.connected {
    background: var(--danger);
  }

  .status {
    color: var(--text-secondary);
    font-size: 13px;
    text-align: center;
  }
</style>
