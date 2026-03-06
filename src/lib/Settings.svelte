<script lang="ts">
  import StatusBar from "./StatusBar.svelte";
  import ServerView from "./ServerView.svelte";
  import ClientView from "./ClientView.svelte";

  let mode: "server" | "client" = $state("server");
  let connected = $state(false);
</script>

<div class="settings">
  <header>
    <h1>Vesa</h1>
    <p class="subtitle">Software KVM</p>
  </header>

  <StatusBar {connected} {mode} info={connected ? "Active" : "Inactive"} />

  <div class="mode-toggle">
    <button
      class:active={mode === "server"}
      onclick={() => (mode = "server")}
    >
      Server
    </button>
    <button
      class:active={mode === "client"}
      onclick={() => (mode = "client")}
    >
      Client
    </button>
  </div>

  <div class="content">
    {#if mode === "server"}
      <ServerView />
    {:else}
      <ClientView />
    {/if}
  </div>
</div>

<style>
  .settings {
    max-width: 400px;
    margin: 0 auto;
  }

  header {
    text-align: center;
    margin-bottom: 20px;
  }

  header h1 {
    font-size: 24px;
    font-weight: 700;
    letter-spacing: -0.5px;
  }

  .subtitle {
    color: var(--text-secondary);
    font-size: 13px;
    margin-top: 2px;
  }

  .mode-toggle {
    display: flex;
    gap: 0;
    margin-bottom: 20px;
    background: var(--bg-secondary);
    border-radius: var(--radius);
    padding: 3px;
  }

  .mode-toggle button {
    flex: 1;
    background: transparent;
    color: var(--text-secondary);
    padding: 8px;
    border-radius: 6px;
  }

  .mode-toggle button:hover {
    transform: none;
    color: var(--text);
  }

  .mode-toggle button.active {
    background: var(--accent);
    color: white;
  }
</style>
