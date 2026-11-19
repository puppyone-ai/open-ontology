/** Least-recently-used map without a library (20-frontend §1.7.2): reading refreshes an entry, writing past `limit` evicts the oldest. */
export class LruMap<K, V> {
  private readonly entries = new Map<K, V>();

  constructor(private readonly limit: number) {
    if (!Number.isInteger(limit) || limit < 1) throw new RangeError("LruMap limit must be a positive integer.");
  }

  get size(): number { return this.entries.size; }

  get(key: K): V | undefined {
    if (!this.entries.has(key)) return undefined;
    const value = this.entries.get(key) as V;
    this.entries.delete(key);
    this.entries.set(key, value);
    return value;
  }

  has(key: K): boolean { return this.entries.has(key); }

  set(key: K, value: V): this {
    this.entries.delete(key);
    this.entries.set(key, value);
    if (this.entries.size > this.limit) this.entries.delete(this.entries.keys().next().value as K);
    return this;
  }

  delete(key: K): boolean { return this.entries.delete(key); }
  clear(): void { this.entries.clear(); }
  keys(): K[] { return [...this.entries.keys()]; }
}
