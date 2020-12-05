# alti2

A communication tool to interact with the Alti-2 Atlas (and possibly N3/N3A), dual-use visual-audible
altimeters that maintain a logbook and speak a weird custom serial protocol.

The only current official way to interact with Alti-2 digital altimeters is via [Paralog](https://paralog.net/),
a third-party (but affiliated to Alti-2) logbook software that costs money and is sold separately the Atlas/N3.
I think the Atlas is a lovely piece of hardware, but I disagree with this stance on software support.

# Status

The tool can currently only complete a successful "Type0" handshake. The focus of functionality will be to
collect jump information and collect it in a database with an open format that can be used by other tools
to provide GUIs or generate graphs/media from the data.

# Past work

My utmost gratefulness to Alexey Lobanov for his [Alti-2 Reader](https://sites.google.com/site/lobanovsoftware/home/alti-2-reader)
software and protocol analysis. Without his work and kind assistance, this project would be much farther off from being useful.

Alexey has written a very helpful [protocol analysis breakdown](https://docs.google.com/viewer?a=v&pid=sites&srcid=ZGVmYXVsdGRvbWFpbnxsb2Jhbm92c29mdHdhcmV8Z3g6MzExMzQ1MjQ5YjFmYjAxNg) of the N3's serial protocol which is great reading.
