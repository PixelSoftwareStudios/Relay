<script lang="ts">
	import { formatDbfs, levelFraction } from "../format";

	let { dbfs, label }: { dbfs: number | null; label: string } = $props();
</script>

<div class="meter" role="meter" aria-label={label} aria-valuemin={-60} aria-valuemax={0} aria-valuenow={dbfs ?? undefined} aria-valuetext={formatDbfs(dbfs)}>
	<div class="fill" style:width="{levelFraction(dbfs) * 100}%"></div>
</div>

<style>
	.meter { height: 6px; width: 120px; background: var(--border); border-radius: 3px; overflow: hidden; }
	.fill { height: 100%; background: var(--live); transition: width 120ms linear; }
</style>
