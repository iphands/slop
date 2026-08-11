# LightDM (1.32.0) — multi-seat facts

All line refs are lightdm-1.32.0 upstream sources. Source tarball on this box:
`/mnt/noir/scratch/gentoo/distfiles/lightdm-1.32.0.tar.xz` (readable without extracting:
`tar -xJOf <tarball> lightdm-1.32.0/src/seat.c`).

## Architecture

One daemon (`lightdm.service`, `BusName=org.freedesktop.DisplayManager`, `Restart=always`)
drives **every** seat. There is no per-seat unit and no supported way to restart one seat as a
service. Seats come from logind (`login1_service_seat_added_cb`); `[Seat:<name>]` config
sections are matched by glob against the logind seat name (`[Seat:*]` applied first).

Objects per seat: a `Seat` → one or more `DisplayServer`s (X, Xmir, Wayland) → `Session`s
(greeter / user). Killing a Session or a DisplayServer is recoverable; a `Seat` stopping is
mostly not.

## The seat0 / seatN asymmetry — the important one

```c
/* src/lightdm.c:418-419, inside add_login1_seat() */
if (is_seat0)                                     /* strcmp(seat_name,"seat0")==0 */
    seat_set_property (seat, "exit-on-failure", "true");
```

Applied **after** `set_seat_properties()` loads the config, and `seat_get_string_property`
reads only the property hash (`src/seat.c:148-153`) that `seat_set_property` just wrote. So
**`exit-on-failure` cannot be turned off for seat0 from lightdm.conf.**

| | seat0 | any other seat |
|---|---|---|
| Seat stops | `display_manager_seat_removed_cb` → `exit_code=FAILURE`, `display_manager_stop` (`src/lightdm.c:257-262`) → daemon exits → systemd `Restart=always` → **all seats restart** | daemon survives, other seats fine, but **the seat is removed permanently** |

Nothing recreates a lost non-seat0 seat at runtime:

- `AddSeat` D-Bus method returns `"AddSeat is deprecated"` (`src/display-manager-service.c:229`).
  `dm-tool add-seat` therefore cannot help. Only `AddLocalXSeat` still works, and it creates an
  `xremote` seat, not a local one.
- The logind re-add path (`update_login1_seat` → `add_login1_seat`, `src/lightdm.c:449-471`) is
  only reachable from `can-graphical-changed`, whose signal is connected **only when
  `logind-check-graphical=true`** (`src/lightdm.c:511-512`).

Recovery is a full daemon restart, which drops every other seat.

## Ways to cycle one seat — none of them are risk-free

| Action | Path | Verdict |
|---|---|---|
| Terminate the seat's **user session** | `src/seat.c:850-858` → `seat_switch_to_greeter` | best available, still risky |
| SIGTERM the seat's **X server** | `src/seat.c:499-511` → `seat_switch_to_greeter` | same risk, no benefit when already at the greeter |
| Kill the **greeter session** when it is the only display server | `src/seat.c:841-848` → `seat_stop` | destroys the seat |
| `loginctl terminate-seat <seat>` | kills the greeter session → as above | destroys the seat |
| `systemctl restart lightdm` | single daemon | drops all seats |

The first two avoid the *immediate* `seat_stop` guard: `display_server_stopped_cb` removes the
server from `priv->display_servers` (`src/seat.c:464`) **before** stopping its sessions, so the
`g_list_length(...)==1 && nth_data(0)==display_server` test at `src/seat.c:843-844` is false by
the time the greeter's `session_stopped_cb` runs, and `is_failed_greeter` is false for an
already-started greeter (`src/seat.c:484`).

**But both still end in `seat_switch_to_greeter`, which calls `seat_stop` if it cannot bring a
greeter up** (`src/seat.c:508-509`, `855-856`). There is no fallback to the greeter that was
already running: `find_greeter_session` skips stopping sessions (`src/seat.c:532`), so lightdm
always tries to construct a *new* display server. If that new X fails to start, the seat dies.

> **Incident, cosmo, 2026-08-10.** seat1 was sitting at the greeter. SIGTERM to its X server
> (`X :1 -layout seat1 -seat seat1`, the path rated "safe" from source reading alone) killed the
> old X; the replacement X did not come up; `seat_stop` ran. lightdm kept running and seat0 was
> untouched — the D-Bus `Seats` property dropped to a single entry — but seat1 was gone and only
> a full `systemctl restart lightdm` brought it back, costing seat0's session anyway. Reading
> the control flow proved which paths *avoid the guard*; it did not prove the replacement
> display server would start. Treat every seat cycle as able to destroy the seat, and arm
> `type=local;local` first.

## `type=a;b` — a spare life for a seat

`type` is a `;`-separated list. On seat removal, `display_manager_seat_removed_cb`
(`src/lightdm.c:224-256`) skips the first entry and builds a replacement seat from the next
one, carrying the name over, then re-adds it. `set_seat_properties` (`src/lightdm.c:139-157`)
copies every config key verbatim into the property hash, so the whole list reaches the seat.

```ini
[Seat:seat1]
type=local;local     # one automatic rebuild if the seat ever stops
```

Each extra `;local` buys one more rebuild. **Read at `add_login1_seat` time**, so editing this
does not arm an already-running seat — it takes effect at the next daemon start.

## D-Bus seat paths are a counter, not a seat name

```c
/* src/display-manager-service.c:481 */
path = g_strdup_printf ("/org/freedesktop/DisplayManager/Seat%d", priv->seat_index);
priv->seat_index++;
```

Registration order, never reset. So the number **has no relation to the logind seat name and
changes after every seat restart**. On the shared box, logind `seat0` was DBus `Seat1` and
logind `seat1` was DBus `Seat0`.

`dm-tool` targets whatever `XDG_SEAT_PATH` holds (`src/dm-tool.c:76-88`) and exits if it is
unset — it has no concept of logind seat names. `dm-tool`'s own `list-seats` prints the DBus
names, so its "Seat0" is not `seat0`. **Address seats with `loginctl` names only.**

`Seat.Sessions` on the bus lists only *user* sessions, so a seat sitting at the greeter shows
`Sessions=0` — it is not a way to tell a live seat from a dead one.

## Useful primitives

```bash
loginctl list-seats --no-legend | awk '{print $1}'
loginctl show-seat seat1 -p Sessions --value        # space-separated session ids
loginctl show-session 45 -p Class -p Name -p Seat -p Display -p State --value
lightdm --show-config                                # combined config, root only
```

Find a seat's root-owned X server by the `-seat <name>` argument pair, never by uid — see
`context/pitfalls.md`, "X server on a multi-seat box is root-owned".

## Config gotchas

- `desired-display-number` appears **nowhere** in upstream 1.32.0 sources nor in Gentoo's
  `lightdm-gentoo-patch-2.tar.gz`. It is a no-op key that a stock-looking `lightdm.conf` may
  carry; display numbers actually come from `[LightDM] minimum-display-number`. Unknown keys
  are copied into the property hash and ignored, so it is harmless — just not doing anything.
- `dbus-service=false` (`src/lightdm.c:856-865`) skips the D-Bus service entirely. Needed if
  you ever run a second instance, since only one process can own the bus name and
  `service_name_lost_cb` calls `exit(EXIT_FAILURE)` (`src/lightdm.c:380-383`). The cost is that
  `dm-tool`, user switching and greeter session-switching stop working for that instance.
- A seat can be excluded from an instance by giving it a `type` no module matches:
  `create_seat` returns NULL → "Unable to create seat" → the seat is simply not managed
  (`src/lightdm.c:403-425`). This is the lever for a one-instance-per-seat split.
