export function charge(orderId: string): Promise<void> {
  return Promise.resolve(orderId).then(() => undefined);
}
