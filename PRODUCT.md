# Product

<!-- impeccable:product-schema 1 -->

## Platform

web (the landing page at webcam.codifyit.dev). The product itself is a macOS desktop app.

## Stack

Landing page: static HTML, CSS and JavaScript in `site/`, no build step. Hosted on Cloudflare
(Workers static assets, like the Clippy site, custom domain `webcam.codifyit.dev`). Release
packages go to a Cloudflare R2 bucket that the owner makes; `/download/mac` redirects to it.

## Users

People who join video meetings from a Mac with a USB or external webcam. They want to look
tidy on calls: a clean or blurred background and a good frame, without new hardware.

## Product Purpose

Webcam Studio sits between the webcam and the meeting app. It blurs or replaces the
background, zooms and mirrors the picture, and gives the result to meeting apps as
"Webcam Studio Camera". Success: a person picks the camera one time in Zoom, Google Meet or
Microsoft Teams and then forgets about it.

## Positioning

macOS already gives external USB webcams background blur, background replacement and
Center Stage. What it does not give external webcams is the zoom and framing control that the built-in
camera has: you cannot zoom in or move the picture to put yourself in the center. Webcam Studio adds zoom, move and mirror, plus its
own background blur and pictures, and sends the framed picture to every meeting app as one
camera. It runs on the device with Apple Vision and is free software (GPL-3.0-or-later).

## Operating Context

- The app lives in the menu bar when its window is closed. A small panel has the meeting
  camera switch, Open Webcam Studio, Settings and Quit.
- The real camera turns on only while a meeting app uses Webcam Studio Camera, and turns off
  about 2 seconds after the meeting app stops.
- One-time setup: install the camera extension, then allow it in System Settings.

## Capabilities and Constraints

Confirmed (shipped in the app today):
- Background: none, blur, or replace with a picture (six drawn presets: Studio, Paper, Sage,
  Dusk, Sand, Slate, or your own picture).
- Cutout controls: edge (cutoff), softness, steadiness. Quality setting.
- Zoom, drag to move the frame, mirror.
- Meeting camera "Webcam Studio Camera" for any meeting app; switch on or off from the menu
  bar; Open at login (off by default).
- Sample videos to try effects without the camera.
- Requirements: macOS 14 or later, Mac with Apple silicon. Signed and notarized by Apple.

Do not claim: Windows or Linux (owner asked for macOS only on the site), virtual microphone,
noise removal (switch exists but is disabled), auto-framing, prices, user counts, reviews.

## Brand Commitments

- Name: Webcam Studio. Mark: viewfinder corners around a person in an orange disc
  (`apps/desktop/src-tauri/icons/source/mark.svg`), accent `#e7783f`, neutral `#202124`.
- Site copy is in ASD-STE100 Simplified Technical English.
- Site visual language: Samsung.com product-page style (owner's explicit brief).
- Publisher: CodifyIt (codifyit.dev), the same as Clippy.

## Evidence on Hand

- App icon and mark in `apps/desktop/src-tauri/icons/`.
- Stock photos: Unsplash License, photographer Vitaly Gariev (see `site/img/CREDITS.md`).
  Effect images on the site are made from these photos with Apple Vision person
  segmentation, the same model family the app uses.
- No testimonials, press, or usage numbers exist. Do not invent them.
- Never use the owner's own camera footage on the site.

## Product Principles

1. Private by default: the picture stays on the Mac; nothing is uploaded.
2. Set it up one time, then forget it.
3. The camera is on only when a meeting uses it.
4. Works with the webcam you already have.
