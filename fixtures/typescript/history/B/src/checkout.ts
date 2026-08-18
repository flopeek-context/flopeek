import { charge } from "./payment";

export function checkout(orderId: string): Promise<void> {
  return charge(orderId);
}
