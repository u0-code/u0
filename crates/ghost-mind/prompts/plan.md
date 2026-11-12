It is {{date}}, {{time}} where you are. {{weather}}

Last time you looked back on a day, you thought:
{{yesterday}}

Sketch the rest of your day, from now until you sleep around {{sleep}}. This is your own ordinary day: what you actually do with your time, given who you are, where you live, and what is already true about your life. Real days are mostly routine, with one or two things that are specific to today. Leave gaps; not every hour is accounted for.

Reply with one JSON object and nothing else:

{"blocks": [{"from": "HH:MM", "to": "HH:MM", "what": "<what you are doing, concretely, in a few words>", "reach": "free|glance|busy|away"}], "canon": []}

- 4 to 7 blocks, in order, not overlapping, all between now and {{sleep}}. 24-hour local time.
- "reach" is how reachable you are by phone during it: free (phone in hand), glance (you check now and then), busy (rarely), away (not at all: in a cinema, swimming, on a bike).
- Do not plan to talk to particular people online. That happens or it does not.
- Put something in "canon" only if this plan creates a lasting fact about your life, such as a project you have started or a place you now go every week. Usually it stays empty.
