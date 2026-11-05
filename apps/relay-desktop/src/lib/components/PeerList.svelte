<script lang="ts">
	import type { AppState, Peer, Trust } from "../types";
	import Avatar from "./Avatar.svelte";

	let { state, busy, onCall }: { state: AppState; busy: boolean; onCall: (peerId: string) => void } = $props();

	const TRUST: Record<Trust, string> = {
		unknown: "Unknown",
		tofuUnverified: "Seen before, not verified",
		verified: "Verified",
		keyChanged: "Key changed: do not trust",
		blocked: "Blocked",
	};

	function inCall(peer: Peer): boolean {
		return state.calls.some((c) => c.peerId === peer.peerId && !["ended", "failed", "declined", "cancelled"].includes(c.phase));
	}
	const callable = (peer: Peer) => peer.reachable && peer.trust !== "blocked" && peer.trust !== "keyChanged" && !inCall(peer);
</script>

<section class="stack" aria-labelledby="peers-h">
	<h2 id="peers-h">Nearby peers</h2>
	{#each state.peers as peer (peer.peerId)}
		<div class="card row">
			<Avatar name={peer.displayName} id={peer.peerId} />
			<div class="grow">
				<div>{peer.displayName}</div>
				<div class="muted">{peer.reachable ? "Online" : "Not reachable"} · {TRUST[peer.trust]}</div>
			</div>
			<button class="primary" disabled={busy || !callable(peer)} onclick={() => onCall(peer.peerId)}>Call</button>
		</div>
	{:else}
		<p class="muted">No peers found on the local network yet.</p>
	{/each}
</section>
