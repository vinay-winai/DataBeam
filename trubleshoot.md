# Troubleshooting DataBeam

## EazySendme/ Sendme specific Transfer Issues
- Transfers will likely fail in networks that actively block p2p traffic like most free vpns. Croc may work in this case as they maintain public relay servers.

## Croc / EazySendme Transfer Issues
- If transfer does not start and logs show room not found or room unavailable/room full/peer left, sender should retry with a different custom code. Avoid using simple codes like 1234567 or aaaaaaa.

## Common UI Issues
- Drag/ Drop is currently not working on linux. So use file / folder buttons instead.
- If app width is small , UI persentation may not be desirable. In that case , please resize the app window.
- progress bar/status is broken, acts funky if retry during export. But don't worry the core functionality is not disrupted. Just an UI issue.

## Suggestions
- Use Standalone Sendme mode for sensitive data transfers.
- the temporary sendme blobs are stored at C:\Users\ < Username > \AppData\Local\Temp and 
  names start with **.sendme-** .
  
