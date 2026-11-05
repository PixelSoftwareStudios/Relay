<script lang="ts">
	import { hueFromId, initials } from "../format";

	let { name, id, src = null, size = 36 }: { name: string; id: string; src?: string | null; size?: number } = $props();
</script>

{#if src}
	<img {src} alt="" width={size} height={size} class="avatar" />
{:else}
	<span
		class="avatar"
		aria-hidden="true"
		style:width="{size}px"
		style:height="{size}px"
		style:background="hsl({hueFromId(id)} 45% 38%)"
		style:font-size="{size * 0.4}px">{initials(name)}</span
	>
{/if}

<style>
	.avatar {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		border-radius: 50%;
		object-fit: cover;
		color: #fff;
		font-weight: 600;
		flex: none;
	}
</style>
