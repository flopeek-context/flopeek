import { charge } from "./payment";

export type CheckoutInput = { orderId: string };

export function checkout(input: CheckoutInput): Promise<void> {
  return charge(input.orderId);
}
