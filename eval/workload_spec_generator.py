import random

# All time intervals are in ms, RPS are in KRPS.
TOTAL_TIME = 5000
START_RPS = 100
TOTAL_RPS_MAX = 1200
MAX_RPS_LIMITS = (200, 800)
PHASE_TIME_INTERVAL_LIMITS = (600, 1400)

RPS_LOWER_LIMIT = 10

RPS_LIMITS = (10, 600)
DURATION_LIMITS = (1, 20)
TRANSITION_LIMITS = (1, 10)

def random_rps_upper_limit():
    return random.randint(MAX_RPS_LIMITS[0], MAX_RPS_LIMITS[1])

def random_phase():
    return random.randint(PHASE_TIME_INTERVAL_LIMITS[0], PHASE_TIME_INTERVAL_LIMITS[1])

def random_rps(rps_upper_limit):
    return random.randint(RPS_LOWER_LIMIT, rps_upper_limit)

def random_duration():
    return random.randint(DURATION_LIMITS[0], DURATION_LIMITS[1])

def random_transition():
    return random.randint(TRANSITION_LIMITS[0], TRANSITION_LIMITS[1])

def calc_total_time(final):
    total_time = 0
    for i in range(0, len(final) - 1, 2):
        total_time += final[i][1] + final[i + 1]
    total_time += final[-1][1]
    return total_time

def format_output(final):
    out = f'{final[0][0]*1000}:{final[0][1]*1000},'
    for i in range(1, len(final), 2):
        out += f'{final[i - 1][0]*1000}-{final[i + 1][0]*1000}:{final[i]*1000},' # transition
        out += f'{final[i + 1][0]*1000}:{final[i + 1][1]*1000},' # level
    return out[:-1]

def main2():
    start_duration = random_duration()
    total_time = TOTAL_TIME - start_duration
    final = [(START_RPS, start_duration)]

    while total_time > 0:
        transition = random_transition()
        rps = random_rps()
        duration = random_duration()

        # No time for new rps, just hold last rps until time ends.
        if total_time - transition < 0:
            (rps, duration) = final[-1]
            final[-1] = (rps, duration + total_time)
            total_time = 0
            break

        # Truncate duration to match exact time.
        if total_time - (transition + duration) < 0:
            final.append(transition)
            total_time -= transition
            
            if total_time != 0:
                final.append((rps, total_time))
                total_time = 0
            
            break

        final.append(transition)
        final.append((rps, duration))
        total_time = total_time - (transition + duration)

    print(format_output(final))

def generate(start_rps, total_time, rps_upper_limit):
    start_duration = random_duration()
    total_time = total_time - start_duration
    final = [(start_rps, start_duration)]

    while total_time > 0:
        transition = random_transition()
        rps = random_rps(rps_upper_limit)
        duration = random_duration()

        # No time for new rps, just hold last rps until time ends.
        if total_time - transition <= 0:
            (rps, duration) = final[-1]
            final[-1] = (rps, duration + total_time)
            total_time = 0
            break

        # Truncate duration to match exact time.
        if total_time - (transition + duration) < 0:
            final.append(transition)
            total_time -= transition
            
            if total_time != 0:
                final.append((rps, total_time))
                total_time = 0
            
            break

        final.append(transition)
        final.append((rps, duration))
        total_time = total_time - (transition + duration)

    return final

def main():
    final1 = []
    final2 = []
    start_rps1 = START_RPS
    start_rps2 = START_RPS
    total_time = TOTAL_TIME

    while total_time > 0:
        phase = min(total_time, random_phase())
        total_time -= phase

        rps_upper_limit = random_rps_upper_limit()
        gen = generate(start_rps1, phase, rps_upper_limit)
        if len(final1) > 0:
            final1[-1] = (final1[-1][0], final1[-1][1] + gen[0][1])
            gen = gen[1:]
        final1.extend(gen)
        start_rps1 = final1[-1][0]

        rps_upper_limit = TOTAL_RPS_MAX - rps_upper_limit
        gen = generate(start_rps2, phase, rps_upper_limit)
        if len(final2) > 0:
            final2[-1] = (final2[-1][0], final2[-1][1] + gen[0][1])
            gen = gen[1:]
        final2.extend(gen)
        start_rps2 = final2[-1][0]

    print(format_output(final1))
    print()
    print(format_output(final2))

if __name__ == '__main__':
    main()
