import random
from test_config import *

def random_rps_upper_limit():
    return random.randint(MAX_RPS_LIMITS[0], MAX_RPS_LIMITS[1])

# def random_phase():
#     return random.randint(PHASE_TIME_INTERVAL_LIMITS[0], PHASE_TIME_INTERVAL_LIMITS[1])

def random_rps(rps_upper_limit):
    return random.randint(max(RPS_LOWER_LIMIT-1, rps_upper_limit - 300), rps_upper_limit)

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
        out += f'{int(final[i - 1][0] * STRESS_FACTOR)*1000}-{int(final[i + 1][0] * STRESS_FACTOR)*1000}:{final[i]*1000},' # transition
        out += f'{int(final[i + 1][0] * STRESS_FACTOR)*1000}:{final[i + 1][1]*1000},' # level
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

    # print(format_output(final))

def generate(start_rps, total_time, rps_upper_limit):
    start_duration = random_duration()
    total_time = total_time - start_duration - DURATION_LIMITS[0]
    final = [DURATION_LIMITS[0], (start_rps, start_duration)]

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
    # print("final: " + str(final))
    return final

def generate_rps_upper_limits():
    limits = [RPS_LOWER_LIMIT] * NUM_BACKENDS
    max_delta = MAX_RPS_LIMITS[1] - RPS_LOWER_LIMIT
    total_rps = TOTAL_RPS_MAX - RPS_LOWER_LIMIT * NUM_BACKENDS
    for i in range(NUM_BACKENDS):
        delta = min(total_rps, random.randint(MAX_RPS_LIMITS[0] - RPS_LOWER_LIMIT, max_delta))
        total_rps -= delta
        limits[i] += delta
        if total_rps < 0:
            break
    
    # Shuffle the limits list
    random.shuffle(limits)
    return limits

def main():
    random.seed(RAND_SEED)
    # Asserts that phase is a factor of total time.
    assert TOTAL_TIME / PHASE_INTERVAL == TOTAL_TIME // PHASE_INTERVAL

    finals = [[(WARMUP_RPS, PHASE_INTERVAL)] for _ in range(NUM_BACKENDS)]
    # start_rps = [START_RPS] * NUM_BACKENDS
    total_time = TOTAL_TIME - PHASE_INTERVAL

    while total_time > 0:
        total_time -= PHASE_INTERVAL
        rps_limits = generate_rps_upper_limits()
        # print(rps_limits)
        # print()
        for i in range(NUM_BACKENDS):
            final = finals[i]
            gen = generate(rps_limits[i], PHASE_INTERVAL, rps_limits[i])
            # if len(final) > 0:
            #     final[-1] = (final[-1][0], final[-1][1] + gen[0][1])
            #     gen = gen[1:]
            final.extend(gen)
            # start_rps[i] = final[-1][0]
            # print(final)
    # print(finals)
    workload_spec = ''
    for final in finals:
        workload_spec = workload_spec + format_output(final) + '/'
    # print(workload_spec[:-1])
    
    return workload_spec[:-1]


if __name__ == '__main__':
    main()
