import Payment from './payment';

export function checkout() {
  const payment = new Payment();
  return payment.charge();
}
