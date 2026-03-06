<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let bindAddr = $state("0.0.0.0:4920");
  let running = $state(false);
  let clients: { name: string; position: string }[] = $state([]);

  async function toggleServer() {
    if (running) {
      await invoke("stop_server");
      running = false;
    } else {
      await invoke("start_server", { bindAddr });
      running = true;
    }
  }
</script>

<div class="server-view">
  <div class="field">
    <label for="bind">Bind Address</label>
    <input id="bind" bind:value={bindAddr} disabled={running} placeholder="0.0.0.0:4920" />
  </div>

  <button class="toggle-btn" class:running onclick={toggleServer}>
    {running ? "Stop Server" : "Start Server"}
  </button>

  {#if running}
    <div class="clients-section">
      <h3>Connected Clients</h3>
      {#if clients.length === 0}
        <p class="empty">No clients connected</p>
      {:else}
        {#each clients as client}
          <div class="client-item">
            <span>{client.name}</span>
            <span class="position">{client.position}</span>
          </div>
        {/each}
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
    font-size: 13px;
    color: var(--text-secondary);
    font-weight: 500;
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

  .toggle-btn.running {
    background: var(--danger);
  }

  .clients-section {
    margin-top: 8px;
  }

  .clients-section h3 {
    font-size: 14px;
    margin-bottom: 8px;
    color: var(--text-secondary);
  }

  .empty {
    color: var(--text-secondary);
    font-size: 13px;
    font-style: italic;
  }

  .client-item {
    display: flex;
    justify-content: space-between;
    padding: 8px 12px;
    background: var(--bg-secondary);
    border-radius: var(--radius);
    margin-bottom: 4px;
  }

  .position {
    color: var(--accent);
    font-size: 13px;
  }
</style>
