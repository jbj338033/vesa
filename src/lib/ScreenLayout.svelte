<script lang="ts">
  type Client = { id: string; name: string; position: string };

  type Props = {
    clients: Client[];
    onpositionchange?: (id: string, position: string) => void;
  };

  let { clients, onpositionchange }: Props = $props();

  const W = 360;
  const H = 200;
  const MW = 100;
  const MH = 64;

  const CX = (W - MW) / 2;
  const CY = (H - MH) / 2;

  const SNAPS: Record<string, { x: number; y: number }> = {
    Left:   { x: CX - MW - 3, y: CY },
    Right:  { x: CX + MW + 3, y: CY },
    Top:    { x: CX, y: CY - MH - 3 },
    Bottom: { x: CX, y: CY + MH + 3 },
  };

  function snap(pos: string) {
    return SNAPS[pos] ?? SNAPS["Right"];
  }

  let dragId = $state<string | null>(null);
  let dragPos = $state({ x: 0, y: 0 });
  let dragOffset = { x: 0, y: 0 };

  function clientPos(client: Client) {
    if (dragId === client.id) return dragPos;
    return snap(client.position);
  }

  function onPointerDown(e: PointerEvent, client: Client) {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const pos = snap(client.position);
    dragId = client.id;
    dragPos = pos;
    dragOffset = { x: e.clientX - pos.x, y: e.clientY - pos.y };
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragId) return;
    dragPos = {
      x: Math.max(0, Math.min(W - MW, e.clientX - dragOffset.x)),
      y: Math.max(0, Math.min(H - MH, e.clientY - dragOffset.y)),
    };
  }

  function onPointerUp() {
    if (!dragId) return;

    const dx = (dragPos.x + MW / 2) - (CX + MW / 2);
    const dy = (dragPos.y + MH / 2) - (CY + MH / 2);

    let result: string;
    if (Math.abs(dx) >= Math.abs(dy)) {
      result = dx < 0 ? "Left" : "Right";
    } else {
      result = dy < 0 ? "Top" : "Bottom";
    }

    onpositionchange?.(dragId, result);
    dragId = null;
  }
</script>

<div class="layout" style="width:{W}px;height:{H}px">
  <div
    class="monitor primary"
    style="left:{CX}px;top:{CY}px;width:{MW}px;height:{MH}px"
  >
    <span>This Mac</span>
  </div>

  {#each clients as client (client.id)}
    {@const pos = clientPos(client)}
    <div
      class="monitor client"
      class:dragging={dragId === client.id}
      style="left:{pos.x}px;top:{pos.y}px;width:{MW}px;height:{MH}px"
      onpointerdown={(e) => onPointerDown(e, client)}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      role="slider"
      aria-label="{client.name} screen position"
      tabindex="0"
    >
      <span>{client.name}</span>
    </div>
  {/each}

  {#if clients.length === 0}
    <div class="empty-hint">
      <span>No clients</span>
    </div>
  {/if}
</div>

<style>
  .layout {
    position: relative;
    background: var(--bg-tertiary);
    border-radius: 10px;
    overflow: hidden;
    border: 1px solid var(--border);
    margin: 0 auto;
  }

  .monitor {
    position: absolute;
    border-radius: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 600;
    user-select: none;
    transition: left 0.2s ease, top 0.2s ease;
  }

  .monitor.dragging {
    transition: none;
  }

  .primary {
    background: var(--accent);
    color: white;
  }

  .client {
    background: var(--bg-secondary);
    color: var(--text);
    border: 1.5px solid var(--text-secondary);
    cursor: grab;
    touch-action: none;
  }

  .client.dragging {
    cursor: grabbing;
    border-color: var(--accent);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    z-index: 1;
  }

  .empty-hint {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
  }

  .empty-hint span {
    font-size: 12px;
    color: var(--text-secondary);
    opacity: 0.5;
  }
</style>
