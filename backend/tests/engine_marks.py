"""Which tests do not run on the app's engine (the new one), and why: the one
place that says so (conftest.py applies it at collection).

TODAY: tests that run today's engine (LIGHTSIM_ENGINE=today, the marker
``todays_engine``), each with its reason:
  (i) a feature the new engine does not run yet and refuses with its code
      (lap cases NEW-LAP, the Formula Student events scored from a lap NEW-FS-EVENT): their tests
      are the oracle for the port (a battery built from cells, ported, runs
      its tests on the new engine);
  (ii) today's engine's own workings (its fixed-step integrator, its
      checks of it, its recorder, its records).
XFAIL: tests the new engine fails today, each an open defect of its path
with its code (``xfail(strict=True)``: a test that starts passing fails, so
its entry is taken out). A key is a test's node id from tests/ ("file.py::
name" for every parameter of a test, or with "[parameter]" for one); a key
that matches no collected test of a collected file fails the run.
"""

LAP = "(i) a lap case (NEW-LAP): not ported yet, today's engine is the oracle"
FS_LAP = ("(i) a Formula Student event scored from a lap case (NEW-FS-EVENT, NEW-LAP): not ported yet, today's "
          "engine is the oracle (the acceleration and an endurance from a speed trace run on the new engine)")
INTEGRATOR = ("(ii) today's engine's own integrator check (its Energy balance residual and Electrical energy "
              "balance error rows; the new engine closes its energy books instead: classes.toml "
              "today-integrator-checks)")
FIXED_STEP = "(ii) today's engine's fixed-step integrator (its error, stability or step choice at its steps)"
POLLS = ("(ii) its Stop lands mid-run only on today's engine, which reads the stop once per output step and runs "
         "slowly enough; the new engine reads it in wall time and ends first (its own Stop tests: "
         "test_new_engine_live.py, test_stop_latency.py)")
WEAK = ("(ii) today's engine's own copy of the weakest group, changed with the new engine's (its charge "
        "factor and its resistance; the both-engines comparison holds the two models together)")

TODAY_FILES: dict[str, str] = {}

# the lap tests that read today's lap energy pass or its live edits within a lap
TODAYS_LAP = ("(ii) today's lap energy pass (lapsim.LapSlave: its braking check, step by step, and a live "
              "edit taken within a lap); the new engine's lap brakes within the brakes and the regeneration "
              "in its profile, and takes a live edit at the next lap's start")

TODAY = {
    "test_lapsim.py::test_lap_braking_keeps_to_the_brakes_and_the_regeneration": TODAYS_LAP,
    "test_lapsim.py::test_braking_the_car_did_not_have_marks_the_lap_not_valid": TODAYS_LAP,
    # (ii) today's engine's own workings
    "test_reference_problems.py::test_todays_engine_stays_within_its_baseline": (
        "(ii) today's engine's own baseline on the reference problems (the new engine's is the engine's harness, engine/)"),
    "test_numerics.py::test_error_halves_when_the_step_halves": FIXED_STEP,
    "test_numerics.py::test_launch_slip_stays_physical": FIXED_STEP,
    "test_numerics.py::test_braked_car_stays_at_rest": FIXED_STEP,
    "test_stability.py::test_no_numerical_slip_across_the_stiffness_range": FIXED_STEP + " (ENG-14)",
    "test_precision.py::test_no_envelope_when_every_point_is_one_solver_step": "(ii) today's recorder: its envelope over the solver steps of an output point",
    "test_precision.py::test_energy_books_are_not_rounded": "(ii) today's energy books, kept every solver step",
    "test_solver.py::test_profiles_are_parsed_once_per_run": "(ii) today's run loop: how often it parses a profile",
    "test_map_edges.py::test_a_run_counts_every_table_it_reads": (
        "(ii) today's RunContext.map_use, its record of each table axis a run reads (the new engine tells "
        "each table read past its data in its own summary rows)"),
    "test_golden.py::test_example_matches_golden[hybrid-car-case-mixed-": (
        "(ii) the golden fixtures are today's engine's regression net (the new engine's is the engine's own golden "
        "comparison: 0 differences in 11,347 values)"),
    "test_golden.py::test_example_matches_golden[bev-car-case-city-": (
        "(ii) the golden fixtures are today's engine's regression net (the new engine's is the engine's own golden "
        "comparison: 0 differences in 11,347 values)"),
    "test_energy.py::test_battery_energy_matches_kinetic_energy": INTEGRATOR,
    "test_battery_charge.py::test_coulombic_efficiency_counts_charge_only": INTEGRATOR,
    "test_energy_book.py::test_each_part_closes_and_all_together_nearly[bev-car-case-city]": INTEGRATOR,
    "test_energy_book.py::test_each_part_closes_and_all_together_nearly[hybrid-car-case-mixed]": INTEGRATOR,
    "test_energy_book.py::test_each_part_closes_and_all_together_nearly[fs-electric-case-accel-75m]": INTEGRATOR,
    "test_energy_book.py::test_a_locked_differential_is_walked_every_step_and_still_closes": INTEGRATOR,
    "test_energy_book.py::test_awd_front_and_rear_final_drives_show_their_own_power": (
        "(ii) today's rule for a gear's power and efficiency (classes.toml gears-today-rule, class b: the new "
        "engine's channel follows the law)"),
    "test_energy_closure.py::test_the_examples_close_their_books": INTEGRATOR,
    "test_energy_closure.py::test_a_motor_spinning_up_an_inertia_closes_its_books": (
        "(ii) today's integrator's books closed to 1e-12 against its own fixed steps"),
    "test_reports.py::test_every_part_keeps_its_books_and_the_car_adds_up": INTEGRATOR,
    "test_reports.py::test_the_examples_energy_adds_up[bev-car-case-city]": INTEGRATOR,
    "test_reports.py::test_the_examples_energy_adds_up[hybrid-car-case-mixed]": INTEGRATOR,
    "test_power_limit.py::test_output_power_limit_holds_terminal_v_times_i": INTEGRATOR,
    "test_source_limits.py::test_min_soc_leaves_the_motor_nothing": INTEGRATOR,
    "test_source_limits.py::test_max_power_point_limits_the_motor": INTEGRATOR,
    "test_source_limits.py::test_fuel_cell_overload_limits_the_motor": INTEGRATOR,
    "test_source_limits.py::test_regen_command_above_the_charge_limit_is_cut": INTEGRATOR,
    "test_verdict.py::test_a_cancelled_run_flags_its_figures_per_distance": POLLS,
    "test_verdict.py::test_a_performance_test_is_timed_only_from_below_its_target_and_to_the_end": POLLS,
    "test_verdict.py::test_a_stop_after_the_last_step_leaves_a_complete_run": POLLS,
    "test_power_limit.py::test_cancelled_run_marks_passes_not_valid": POLLS,
    "test_studies.py::test_the_live_channel_streams_points_and_stops": POLLS,
    "test_script_sandbox.py::test_worker_is_gone_after_a_cancelled_run": POLLS,
    "test_docs.py::test_the_validation_pages_table_is_what_the_suite_measures": (
        "(ii) the validation page records today's engine's measurement (20 October 2026); measuring it again on "
        "the new engine is the documents' piece"),
    "test_docs.py::test_the_validation_pages_summary_lines_agree_with_its_table": (
        "(ii) the validation page records today's engine's measurement (20 October 2026); its coast-down runs "
        "are refused on the new engine (SINGULAR-LOOP, below)"),
    "test_docs.py::test_what_the_validation_page_says_about_the_suites_cars_is_true": (
        "(ii) the validation page records today's engine's measurement (20 October 2026)"),
    "test_docs.py::test_everything_that_says_how_far_lightsim_is_validated_gives_the_suites_figure": (
        "(ii) the validation page records today's engine's measurement (20 October 2026)"),
    # (ii) today's engine's own battery model, changed with the new engine's (one model in both)
    "test_battery_cells.py::test_a_weak_group_fills_first_and_the_charge_band_follows_it[today]": WEAK,
    "test_battery_cells.py::test_a_weak_groups_resistance_is_read_at_its_own_soc[today]": WEAK,
}

TICK = ("a sampled block's (Script, FMU, Delay) value at an output point is the one after its tick there, where "
        "today's shows the one before (the blocks piece's open item; changing it moves the hybrid's golden values)")



XFAIL = {
    "test_lapsim.py::test_75_m_straight_matches_the_time_domain_within_2_percent[2.5-vehicle2-wheel2]": (
        "NEW-LAP-SLIP: where the motors at full torque hold the car, the lap's wheel (fast mode's inverse "
        "model) turns with the prescribed speed and takes its tyre's slip loss out of the force: 3.835 s "
        "against the time-domain 3.706 s at μ 2.5, about the slip at that force (10 %) (open)"),
    "test_control_rate.py::test_block_with_a_sample_time_runs_at_that_rate_and_holds": TICK,
    "test_signal_wiring.py::test_the_delay_is_what_the_run_does": TICK,
    "test_road_load.py::test_ambient_sets_the_air_density": "the speed trace under a changed air density differs from today's (open)",
    "test_solver.py::test_diff_open_torque_split_is_equal": "an open differential's split is equal to 3e-6 on the new engine, the test asks 1e-6 (open)",
    "test_battery_charge.py::test_a_1c_discharge_empties_the_battery_in_one_hour": (
        "the current between the solver's points, interpolated across a step that spans the OCV table's "
        "kink (read at the SOC, stepped over: engine DESIGN 8.2), is 0.08 % off 1C at the table's 90 % "
        "and 10 % (steps of 5 to 10 s); the SOC and the time to empty are right (open)"),
}
