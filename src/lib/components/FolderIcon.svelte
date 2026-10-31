<script lang="ts">
  let { size = 64 }: { size?: number } = $props();
</script>

<svg width={size} height={(size * 58) / 72} viewBox="0 0 72 58" fill="none" aria-hidden="true">
  <path
    d="M4 9a5 5 0 0 1 5-5h16.5a5 5 0 0 1 3.6 1.5L33 9.5h30a5 5 0 0 1 5 5V49a5 5 0 0 1-5 5H9a5 5 0 0 1-5-5z"
    fill="var(--folder-back)"
  />
  <path
    d="M4 19a5 5 0 0 1 5-5h54a5 5 0 0 1 5 5v30a5 5 0 0 1-5 5H9a5 5 0 0 1-5-5z"
    fill="var(--folder-front)"
  />
  <path d="M9 14h54a5 5 0 0 1 5 5v1H4v-1a5 5 0 0 1 5-5z" fill="#fff" fill-opacity="0.3" />
</svg>
