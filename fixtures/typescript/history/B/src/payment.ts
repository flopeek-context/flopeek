export async function charge(orderId: string): Promise<void> {
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      await Promise.resolve(orderId);
      return;
    } catch {
      if (attempt === 1) throw new Error("payment failed");
    }
  }
}
