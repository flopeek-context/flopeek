export async function charge(orderId: string): Promise<void> {
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      await Promise.resolve(orderId);
      return;
    } catch {
      if (attempt === 1) {
        await Promise.race([Promise.resolve(orderId), new Promise((resolve) => setTimeout(resolve, 1000))]);
        throw new Error("payment timeout");
      }
    }
  }
}
