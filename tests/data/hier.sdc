create_clock -name clk -period 10 [get_ports clk]
set_input_delay 1 -clock clk [all_inputs]
set_output_delay 1 -clock clk [all_outputs]
set_false_path -to [get_pins u_b/r/D]
