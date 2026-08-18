import { checkout } from './checkout';
import { cycleA } from './cycle-a';

export function main() {
  cycleA();
  return checkout();
}

main();
