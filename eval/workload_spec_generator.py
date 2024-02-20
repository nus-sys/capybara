import random

# All time intervals are in ms, RPS are in KRPS.
TOTAL_TIME = 5000
START_RPS = 100
RPS_LIMITS = (10, 600)
DURATION_LIMITS = (1, 20)
TRANSITION_LIMITS = (1, 10)

def random_rps():
    return random.randint(RPS_LIMITS[0], RPS_LIMITS[1])

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

def main():
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

if __name__ == '__main__':
    main()
