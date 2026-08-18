import { checkout } from "./checkout";

export function CheckoutButton(): JSX.Element {
  return <button onClick={() => checkout({ orderId: "demo" })}>Pay</button>;
}
