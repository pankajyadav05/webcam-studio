export const PERSON_OUTLINE = 'M 451 720 L 466 642 C 402 638 375 615 379 573 C 384 518 400 462 410 416 C 419 376 422 342 448 321 C 479 299 537 276 574 255 L 579 219 C 563 201 554 176 551 150 C 542 145 539 123 550 117 C 546 83 551 54 577 34 C 603 14 636 17 657 23 C 693 31 708 59 710 109 C 726 113 724 136 712 151 C 708 182 695 208 679 222 L 681 253 C 733 278 793 284 824 316 C 844 335 834 367 828 396 C 834 442 832 527 804 581 L 791 633 L 802 720 Z';
// A traced portrait cutout makes the dark headrest independent of the photo's backdrop.
export function chairFixture(image) {
  const person = document.createElement('canvas'); person.width = 1280; person.height = 720;
  const p = person.getContext('2d');
  const outline = new Path2D(PERSON_OUTLINE);
  p.clip(outline);
  const width=720*image.width/image.height;p.drawImage(image,(1280-width)/2,0,width,720);
  return person;
}
export function drawChair(ctx, person, tick = 0) {
  ctx.fillStyle='#919794';ctx.fillRect(0,0,1280,720);
  const shift=Math.sin(tick/40)*12;
  ctx.save();ctx.translate(shift,0);
  ctx.fillStyle='#15171b';ctx.beginPath();ctx.roundRect(425,110,455,565,70);ctx.fill();
  ctx.fillStyle='#282b31';ctx.beginPath();ctx.roundRect(493,60,294,130,32);ctx.fill();
  ctx.strokeStyle='#474b54';ctx.lineWidth=5;ctx.stroke();
  ctx.fillStyle='#090b0f';ctx.fillRect(542,90,194,52);
  ctx.drawImage(person,0,0);ctx.restore();
}
