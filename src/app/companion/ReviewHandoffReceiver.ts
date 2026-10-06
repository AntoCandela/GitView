/** Applies only a current native claim; discovery and successful window focus convey no mutation authority. */
import type { ReviewHandoffClient, ReviewHandoffTarget } from "../../contracts/companion";

export class ReviewHandoffReceiver {
  private generation = 0;
  private revision = -1;
  private contextEpoch = "";
  private requestId: string | null = null;
  private disposed = false;
  private prepared = false;

  constructor(private readonly client: ReviewHandoffClient,
    private readonly apply: (target: ReviewHandoffTarget, contextEpoch: string, requestId: string) => void,
    private readonly now: () => number = () => performance.now()) {}

  reconcile(contextEpoch: string, revision: number, requestId: string | null, prepared = true) {
    if (this.disposed || revision < this.revision) return;
    if (this.contextEpoch === contextEpoch && this.revision === revision && this.requestId === requestId && this.prepared === prepared) return;
    this.contextEpoch = contextEpoch;
    this.revision = revision;
    this.requestId = requestId;
    this.prepared = prepared;
    const generation = ++this.generation;
    if (prepared && requestId !== null) void this.receive(generation, contextEpoch, requestId);
  }

  dispose() { this.disposed = true; ++this.generation; }

  private async receive(generation: number, contextEpoch: string, requestId: string) {
    try {
      const discovery = await this.client.pending();
      if (this.disposed || generation !== this.generation || discovery.revision < this.revision) return;
      if (discovery.revision > this.revision || discovery.pending?.requestId !== requestId) {
        this.reconcile(contextEpoch, discovery.revision, discovery.pending?.requestId ?? null, this.prepared);
        return;
      }
      if (!discovery.pending || discovery.pending.contextEpoch !== contextEpoch || discovery.pending.phase !== "pending") return;
      const invokedAt = this.now();
      const claim = await this.client.claim(requestId, contextEpoch);
      // Native samples remaining time before replying. Invocation time is deliberately conservative.
      if (claim.kind !== "claimed" || this.disposed || generation !== this.generation ||
        this.contextEpoch !== contextEpoch || this.requestId !== requestId || claim.requestId !== requestId ||
        claim.contextEpoch !== contextEpoch || claim.handoffRevision < this.revision ||
        !Number.isFinite(claim.remainingMs) || claim.remainingMs <= 0 || this.now() >= invokedAt + claim.remainingMs) return;
      this.revision = claim.handoffRevision;
      this.apply(claim.target, contextEpoch, requestId);
      // No await/effect/deferred transition is permitted between the final guard and synchronous apply.
      await this.client.ack(requestId, contextEpoch,
        claim.target?.kind === "no_remaining" ? "changed" : claim.target?.kind === "unavailable" ? "unavailable" : "applied");
    } catch {
      // No acknowledgement means the host keeps/restores the source and reports delivery failure.
    }
  }
}
