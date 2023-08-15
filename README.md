# fuck this, not gonna maintain it anymore. old readme below






# Ayaled - A small and cute tool to control leds on Aya Neo Devices

## API:
Everything is controlled via HTTP requests.
Endpoints:

`GET 127.0.0.1:21371/set/{mode}/{r}/{g}/{b}` - Sets color for `mode`
`GET 127.0.0.1:21371/get/{mode}` - Gets color for `mode` in `{r}:{g}:{b}\n` format

With `mode` being:
- `charging`, meaning device charging and below 90% battery capacity
- `low_bat`, meaning device having battery capacity between 0 and 20%
- `full`, meaning device having battery capacity of at least 90%
- `normal`, covering all other cases

And `r`, `g` and `b` being RGB values between 0 and 255.

Keep in mind that all mode values are for a case with 100% brightness,
when the brightness of main display is lowered, the brightness values get
scaled accordingly.

### Example API calls
- `curl 127.0.0.1:21371/set/normal/200/0/200` sets "normal" color to purple
- `curl 127.0.0.1:21371/set/charging/255/0/0` sets "charging" color to red
- `curl 127.0.0.1:21371/get/charging` gets "charging" color

### Errors:
- `400 Bad Request` - Invalid mode was chosen
- `404 Not Found` - Either invalid endpoint or value out of range
