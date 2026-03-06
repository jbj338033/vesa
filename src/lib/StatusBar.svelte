<script lang="ts">
  type Props = {
    connected: boolean;
    mode: "server" | "client";
    info: string;
  };

  let { connected, mode, info }: Props = $props();
</script>

<div class="status-bar">
  <div class="indicator" class:connected></div>
  <span class="mode">{mode === "server" ? "Server" : "Client"}</span>
  <span class="info">{info}</span>
</div>

<style>
  .status-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
    background: var(--bg-secondary);
    border-radius: var(--radius);
    margin-bottom: 16px;
  }

  .indicator {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--danger);
    transition: background 0.3s ease;
  }

  .indicator.connected {
    background: var(--success);
  }

  .mode {
    font-weight: 600;
    font-size: 14px;
  }

  .info {
    color: var(--text-secondary);
    font-size: 13px;
    margin-left: auto;
  }
</style>
