create_clock -name clk -period 10 [get_ports clk]
set_input_delay 1 -clock clk [all_inputs]
set_output_delay 1 -clock clk [all_outputs]
set_clock_gating_check -setup 0.1 [get_clocks clk]
set_clock_gating_check -hold 0.1 [get_clocks clk]
set_max_area 0
