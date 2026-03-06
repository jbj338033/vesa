<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import ServerView from "./ServerView.svelte";
  import ClientView from "./ClientView.svelte";

  let mode: "server" | "client" = $state("server");
  let active = $state(false);

  onMount(async () => {
    try {
      const status = await invoke<string>("get_status");
      if (status === "server") {
        mode = "server";
        active = true;
      } else if (status === "client") {
        mode = "client";
        active = true;
      }
    } catch {}
  });
</script>

<div class="settings">
  <div class="titlebar" data-tauri-drag-region>
    <span class="title">Vesa</span>
  </div>

  <div class="mode-toggle">
    <button
      class:active={mode === "server"}
      disabled={active && mode !== "server"}
      onclick={() => (mode = "server")}
    >
      Server
    </button>
    <button
      class:active={mode === "client"}
      disabled={active && mode !== "client"}
      onclick={() => (mode = "client")}
    >
      Client
    </button>
  </div>

  <div class="content">
    {#if mode === "server"}
      <ServerView bind:running={active} />
    {:else}
      <ClientView bind:connected={active} />
    {/if}
  </div>
</div>

<style>
  .settings {
    max-width: 440px;
    margin: 0 auto;
    padding: 0 24px 24px;
  }

  .titlebar {
    text-align: center;
    padding: 16px 0 20px;
  }

  .title {
    font-size: 15px;
    font-weight: 700;
    letter-spacing: -0.02em;
    color: var(--text);
  }

  .mode-toggle {
    display: flex;
    background: var(--bg-secondary);
    border-radius: 10px;
    padding: 3px;
    margin-bottom: 24px;
  }

  .mode-toggle button {
    flex: 1;
    background: transparent;
    color: var(--text-secondary);
    padding: 8px;
    border-radius: 8px;
    font-size: 13px;
  }

  .mode-toggle button:hover:not(:disabled) {
    color: var(--text);
  }

  .mode-toggle button.active {
    background: var(--bg-tertiary);
    color: var(--text);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
  }

  .mode-toggle button:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }
</style>
