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

PORT = ("an acceleration case of a car whose Driver has Target Speed or Actual Speed linked was refused "
        "(PORT-NOT-MAPPED, fixed: it runs now)")
MAX_TABLE = ("NEW-FAILED (RUN-TABLE): a motor driven past its maximum speed reads its full-load map past its "
             "speed data and the run stops there (open)")
TABLE_WORDS = ("NEW-FAILED (RUN-TABLE): the new engine stops the run where today's does, but its message names the "
               "table by its internal name at the engine's time, not in today's words the test reads (open)")
TICK = ("a sampled block's (Script, FMU, Delay) value at an output point is the one after its tick there, where "
        "today's shows the one before (the blocks piece's open item; changing it moves the hybrid's golden values)")
LIVE_TABLE = "NEW-LIVE-NEXT-RUN: a live edit of a table takes effect on the next run on the new engine (open)"


LIMITS = ("NEW-LIMITS: the new path refuses a run with a value outside its parameter's limits (Data Checks refuse "
          "{what}), where today's engine clamps it and runs; a refusal that is not one of the four features (open: "
          "the owner's call whether the app keeps it)")

XFAIL = {
    "test_lapsim.py::test_75_m_straight_matches_the_time_domain_within_2_percent[2.5-vehicle2-wheel2]": (
        "NEW-LAP-SLIP: where the motors at full torque hold the car, the lap's wheel (fast mode's inverse "
        "model) turns with the prescribed speed and takes its tyre's slip loss out of the force: 3.835 s "
        "against the time-domain 3.706 s at μ 2.5, about the slip at that force (10 %) (open)"),
    "test_lab_figures.py::test_ftp_bags_are_weighted_as_epa_does": (
        "the new path gives no 'FTP weighted fuel consumption' row: the engine's figures have each bag, today's "
        "lab_rows weights them (open)"),
    "test_acceleration.py::test_the_run_ends_at_the_line_and_is_timed_inside_the_last_step": PORT + (
        "; it still differs: 5.5463 s against 5.533 +- 0.002 s (open)"),
    "test_acceleration.py::test_the_start_line_starts_the_timer_and_the_gap_is_signed": PORT + (
        "; it still fails: StopIteration in the test's own _at (open)"),
    "test_live_params.py::test_live_edit_survives_two_gear_shifts": (
        "the test counts the older engine's shift rebuilds (_spy_shifts), which the new engine does not make: "
        "it sees none (open)"),
    "test_map_edges.py::test_a_motor_driven_past_its_maximum_speed_is_flagged": MAX_TABLE,
    "test_verdict.py::test_a_motor_above_its_maximum_speed_is_named": MAX_TABLE,
    "test_map_edges.py::test_maximum_speed_beyond_the_map_stops_the_run": TABLE_WORDS,
    "test_map_edges.py::test_a_lookup_set_to_error_stops_the_run": TABLE_WORDS,
    "test_map_edges.py::test_a_loss_map_narrower_than_the_full_load_map_stops_the_run": TABLE_WORDS,
    "test_map_edges.py::test_a_run_an_error_stops_has_its_figures_marked_not_valid": (
        "NEW-FAILED (RUN-TABLE): a run the new engine stops on a table gives no partial result, where today's "
        "keeps what ran with its figures marked not valid (open)"),
    "test_map_edges.py::test_300_kmh_target_stops_at_the_motors_maximum_speed": (
        "a motor at its maximum speed: the new path does not write today's note that the motor holds the car "
        "there (classes.toml new-path-lacks-messages, class a; open)"),
    "test_map_edges.py::test_maximum_speed_below_the_map_is_a_speed_limiter": (
        "a motor at its maximum speed: the new path does not write today's note that the motor holds the car "
        "there (classes.toml new-path-lacks-messages, class a; open)"),
    "test_map_edges.py::test_max_speed_is_live_tunable": (
        "a live edit of a motor's maximum speed: the new engine keeps the speed the run started with "
        "(11,926 1/min against the edited 9,000; open)"),
    "test_map_edges.py::test_clamp_and_linear_lookup_blocks_count_their_time_outside": (
        "a Lookup block's time outside its data on Clamp or Linear is not said as today's says it (open)"),
    "test_map_edges.py::test_a_run_counts_every_table_it_reads": (
        "the new path's count of the tables a run reads past their data differs from today's (open)"),
    "test_reference_suite.py::test_exact_answer_tier[coast-": (
        "NEW-FAILED (SINGULAR-LOOP): the reference suite's coast-down model ('Brake RL' and 'Wheel RL') is a loop "
        "the new engine cannot solve with its values (open)"),
    "test_source_limits.py::test_battery_driven_to_its_limits_closes_the_energy_balance": (
        "NEW-FAILED (RUN-INIT): the new engine cannot re-initialise the model at t = 2.23 s (open)"),
    "test_source_limits.py::test_empty_battery_sheds_its_consumers": (
        "NEW-FAILED (RUN-INIT): the new engine cannot re-initialise the model at t = 2.18 s (open)"),
    "test_source_limits.py::test_one_way_dcdc_passes_no_regen_upstream": (
        "the new path writes no Electrical energy balance error row, today's share of its last-resort clamps "
        "(the converter, the motor and the consumer behind it are as the test asks; open)"),
    "test_source_limits.py::test_full_battery_regen_goes_to_the_friction_brakes": "a full battery's regeneration: the numbers differ from today's (open)",
    "test_source_limits.py::test_driver_recuperates_up_to_the_charge_limit_and_brakes_the_rest": (
        "the charge power ends 1.5e-4 kW over the charge limit on the new engine (open)"),
    "test_source_limits.py::test_a_full_battery_recuperates_nothing_under_the_driver": (
        "the new path writes a 'regeneration not recovered' row for a full battery (open)"),
    "test_verdict.py::test_non_finite_values_fail_the_run": (
        "NEW-FAILED (RUN-NOT-FINITE): the new engine stops at the first value that is not a number, in its own "
        "words, where today's says 'Non-finite values' naming the channel (open)"),
    "test_climate.py::test_a_live_edit_of_the_demand_table_applies": LIVE_TABLE,
    "test_battery_cells.py::test_a_live_edit_of_the_cell_voltage_curve_applies": LIVE_TABLE,
    "test_solver.py::test_live_profile_edit_takes_effect": LIVE_TABLE,
    "test_control_rate.py::test_scripts_run_every_solver_step_with_its_dt": TICK,
    "test_control_rate.py::test_block_with_a_sample_time_runs_at_that_rate_and_holds": TICK,
    "test_signal_wiring.py::test_the_delay_is_what_the_run_does": TICK,
    "test_fmu.py::test_communication_step_holds_outputs_between_steps": TICK,
    "test_gear_shifts.py::test_a_shift_keeps_the_angular_momentum_and_books_the_energy_it_loses": (
        "a gear shift's energy on the new engine differs from the rigid engagement the test computes (open)"),
    "test_gear_shifts.py::test_through_gripping_tyres_the_vehicle_takes_part_in_the_shift": "a gear shift through gripping tyres differs from today's (open)",
    "test_gear_shifts.py::test_an_engine_behind_a_closed_clutch_keeps_its_speed_and_the_clutch_slips": "a closed clutch's slip differs from today's (open)",
    "test_live_params.py::test_drivelines_shift_in_the_same_step": "two drivelines' shifts on the new engine do not fall in the same step (open)",
    "test_live_params.py::test_the_result_lists_the_live_edits_it_made": (
        "a live edit read at the start is listed at the run's first output time (10 s), where today's lists it at "
        "t = 0 (open)"),
    "test_load_transfer.py::test_axle_loads_follow_m_a_h_over_l": "the axle loads differ from m a h / L on the new engine (open)",
    "test_load_transfer.py::test_a_lifting_axle_carries_nothing": "the axle loads differ on the new engine (open)",
    "test_load_transfer.py::test_the_slope_moves_load_to_the_rear": "the axle loads on a slope differ on the new engine (open)",
    "test_mcp_server.py::test_every_answer_stays_under_20_kb": "an MCP answer of a new-engine run is 20,027 bytes, over the 20 kB the server keeps to (open)",
    "test_numerics.py::test_coasting_up_a_grade_turns_kinetic_energy_into_m_g_h": "coasting up a grade: 16.66 against 20.39 (m g h) on the new engine (open)",
    "test_numerics.py::test_clutch_engagement_loses_the_two_inertia_energy": "a clutch on the new engine never locks exactly (slip 0.0) in the test's window (open)",
    "test_power_limit.py::test_usable_energy_left": "an idle car's final SOC is 89.9991 % on the new engine, not exactly its 90 % (open)",
    "test_references.py::test_energy_from_nowhere_is_flagged": "the new path grades energy from nowhere 'within', where today's grades it 'outside' (open)",
    "test_reports.py::test_a_propeller_keeps_its_books": "a propeller's books on the new path: losses 0.090 against energy in 0.146 kWh (open)",
    "test_road_load.py::test_ambient_sets_the_air_density": "the speed trace under a changed air density differs from today's (open)",
    "test_solver.py::test_diff_open_torque_split_is_equal": "an open differential's split is equal to 3e-6 on the new engine, the test asks 1e-6 (open)",
    "test_traction_control.py::test_slip_holds_its_target_after_the_launch_ramp": "Traction Control's slip after the launch ramp is 0.359 on the new engine, over 0.3 (open)",
    "test_traction_control.py::test_the_defaults_stay_stable_at_the_10_ms_step": "Traction Control's run differs: 3.756 against 3.744 +- 0.01 (open)",
    "test_vehicle_stop.py::test_a_coasting_car_stops_where_the_closed_form_says_and_stays_stopped": "a coasting car's speed never reaches exactly 0 on the new engine (open)",
    "test_vehicle_stop.py::test_rolling_resistance_holds_a_car_on_a_slope_it_cannot_overcome": "a car held by rolling resistance moves -5.9e-14 on the new engine, not exactly 0 (open)",
    "test_verdict.py::test_a_car_that_does_not_move_fails_and_says_so_early": "the new engine does not stream the early message that the car does not move (open)",
    "test_power_limit.py::test_a_margin_outside_0_to_100_percent_is_refused_not_a_crash": LIMITS.format(
        what="it for the case as well"),
    "test_road_load.py::test_a_case_value_outside_the_usual_air_is_warned_about_by_the_run[case_values2-": LIMITS.format(
        what="no case value of the Ambient; today's run warns"),
    "test_load_transfer.py::test_a_wheelbase_of_0_given_at_run_time_is_warned_about": LIMITS.format(
        what="no case value of the Vehicle; today's run warns"),
    "test_battery_charge.py::test_a_1c_discharge_empties_the_battery_in_one_hour": LIMITS.format(
        what="a Minimum SOC of 0 for a single run too"),
    "test_battery_charge.py::test_without_a_charge_capacity_it_comes_from_the_usable_capacity": LIMITS.format(
        what="a Minimum SOC of 0 for a single run too"),
    "test_numerics.py::test_rc_branch_step_response_is_first_order": LIMITS.format(
        what="a Minimum SOC of 0 for a single run too"),
    "test_numerics.py::test_error_halves_when_the_step_halves[battery RC (implicit)]": LIMITS.format(
        what="a Minimum SOC of 0 for a single run too"),
}
