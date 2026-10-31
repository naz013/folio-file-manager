<script lang="ts">
  let { name, size = 48 }: { name: string; size?: number } = $props();

  const badgeColors: Record<string, string> = {
    pdf: "#E5484D",
    doc: "#2F6FEB",
    docx: "#2F6FEB",
    xls: "#1F9D55",
    xlsx: "#1F9D55",
    fig: "#A259FF",
    zip: "#8E8E93",
    md: "#1D1D22",
  };

  let ext = $derived(name.includes(".") ? name.split(".").pop()!.toLowerCase() : "");
  let badge = $derived(ext.slice(0, 4).toUpperCase());
  let color = $derived(badgeColors[ext] ?? "#8E8E93");
</script>

<svg width={(size * 56) / 72} height={size} viewBox="0 0 56 72" fill="none" aria-hidden="true">
  <path
    d="M7 1.5h31L54.5 18v48A4.5 4.5 0 0 1 50 70.5H7A4.5 4.5 0 0 1 2.5 66V6A4.5 4.5 0 0 1 7 1.5z"
    fill="#fff"
    stroke="#D9D9E1"
  />
  <path d="M38 1.5V14a4 4 0 0 0 4 4h12.5" fill="#F2F2F6" stroke="#D9D9E1" />
  <rect x="10" y="24" width="28" height="2.5" rx="1.25" fill="#E3E3EA" />
  <rect x="10" y="30" width="36" height="2.5" rx="1.25" fill="#E3E3EA" />
  <rect x="10" y="36" width="32" height="2.5" rx="1.25" fill="#E3E3EA" />
  {#if badge}
    <rect x="6" y="50" width={Math.max(20, badge.length * 7 + 8)} height="16" rx="4" fill={color} />
    <text
      x={6 + Math.max(20, badge.length * 7 + 8) / 2}
      y="61.5"
      text-anchor="middle"
      font-size="9"
      font-weight="700"
      fill="#fff">{badge}</text
    >
  {/if}
</svg>
